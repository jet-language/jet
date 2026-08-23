use jet_codegen::Codegen::TIR::{
    self, JitProgram, JitSpawnCapture, TBuiltinOp, TCallArg, TCoreClosureKind, TEnumPayload, TExpr,
    TExprKind, TFunc, TFuncKind, THandleOp, THostCall, TIfCond, TJitSpawnBody, TJitSpawnLambda, TModuleCallForm, TOrFallback,
    ListSpreadPart, TStmt, TStrPart,
    TNumericOp,
};
use jet_foundation::AST::{BinOp, Pattern, Type, UnOp};
use std::collections::HashSet;

pub(crate) fn entry_return_supported(ret: Option<&Type>) -> bool {
    ret.is_none()
        || matches!(ret, Some(Type::Named(name)) if name == "App")
        || matches!(ret, Some(Type::Result { ok, err })
            if matches!(ok.as_ref(), Type::Named(name) if name == "Unit" || name == "App")
                && entry_error_supported(err))
}

fn entry_error_supported(err: &Type) -> bool {
    match err {
        Type::String | Type::Named(_) => true,
        Type::Union(members) => !members.is_empty() && members.iter().all(entry_error_supported),
        Type::List(inner) => matches!(inner.as_ref(), Type::Named(name) if name == "FieldError"),
        _ => false,
    }
}

pub(crate) fn flatten_string(parts: &[TStrPart]) -> Option<String> {
    let mut out = String::new();
    for p in parts {
        match p {
            TStrPart::Lit(s) => out.push_str(s),
            TStrPart::Interp(_, _) => return None,
        }
    }
    Some(out)
}

fn resident_safe_string_parts(parts: &[TStrPart], callees: &HashSet<String>) -> bool {
    parts.iter().all(|p| match p {
        TStrPart::Lit(_) => true,
        TStrPart::Interp(e, _) => resident_safe_expr(e, callees),
    })
}

fn resident_safe_compute_call(
    method: &str,
    args: &[TExpr],
    result_ty: &Type,
    callees: &HashSet<String>,
) -> bool {
    match (method, args) {
        ("gradient" | "value_and_gradient" | "vjp" | "jvp", args)
            if args.len() >= 2
                && matches!(args.last().map(|arg| &arg.ty), Some(Type::List(inner)) if matches!(inner.as_ref(), Type::Int)) =>
        {
            let function = &args[0];
            let Type::Fn { params, ret, .. } = &function.ty else {
                return false;
            };
            let valid_params = params
                .iter()
                .all(|param| param.is_compute_tensor_family());
            let valid_result = ret.as_deref().is_some_and(|result| {
                result.is_compute_tensor_family()
                    || (method == "gradient"
                        && matches!(result, Type::Tuple(fields) if fields
                            .iter()
                            .all(|(_, field)| field.is_compute_tensor_family())))
            });
            let valid_transform_result = resident_safe_compute_result(method, args, result_ty);
            let value_count = args.len().saturating_sub(2);
            let expected_values = if value_count == 0 {
                0
            } else if method == "jvp" {
                params.len().saturating_mul(2)
            } else {
                params.len()
            };
            value_count == expected_values
                && valid_params
                && valid_result
                && valid_transform_result
                && args.iter().all(|arg| resident_safe_expr(arg, callees))
        }
        ("from_list", [values]) if jit_list_float_type(&values.ty) => {
            resident_safe_expr(values, callees)
        }
        ("vec", [len, fill])
            if intish_ty(&len.ty)
                && matches!(erase_runtime_qualifiers(&fill.ty), Type::Float | Type::Float32) =>
        {
            args.iter().all(|arg| resident_safe_expr(arg, callees))
        }
        ("matrix", [rows, cols, fill])
            if intish_ty(&rows.ty)
                && intish_ty(&cols.ty)
                && matches!(erase_runtime_qualifiers(&fill.ty), Type::Float | Type::Float32) =>
        {
            args.iter().all(|arg| resident_safe_expr(arg, callees))
        }
        ("zeros" | "ones", [shape]) if jit_list_int_type(&shape.ty) => {
            resident_safe_expr(shape, callees)
        }
        ("full", [shape, value])
            if jit_list_int_type(&shape.ty)
                && matches!(erase_runtime_qualifiers(&value.ty), Type::Float | Type::Float32) =>
        {
            args.iter().all(|arg| resident_safe_expr(arg, callees))
        }
        ("eye", [size]) if intish_ty(&size.ty) => resident_safe_expr(size, callees),
        ("reshape", [tensor, shape])
            if tensor.ty.is_compute_tensor_family() && jit_list_int_type(&shape.ty) =>
        {
            args.iter().all(|arg| resident_safe_expr(arg, callees))
        }
        ("device_cpu" | "device_auto" | "device_metal" | "device_vulkan" | "device_webgpu", []) => true,
        ("on_device", [tensor, device])
            if tensor.ty.is_compute_tensor_family() && jit_value_type(&device.ty) =>
        {
            args.iter().all(|arg| resident_safe_expr(arg, callees))
        }
        ("broadcast_to", [tensor, shape])
            if tensor.ty.is_compute_tensor_family() && jit_list_int_type(&shape.ty) =>
        {
            args.iter().all(|arg| resident_safe_expr(arg, callees))
        }
        ("transpose" | "det" | "inv" | "fft", [tensor])
            if tensor.ty.is_compute_tensor_family() =>
        {
            resident_safe_expr(tensor, callees)
        }
        ("solve", [left, right])
            if left.ty.is_compute_tensor_family() && right.ty.is_compute_tensor_family() =>
        {
            args.iter().all(|arg| resident_safe_expr(arg, callees))
        }
        ("stream_new", []) => true,
        ("stream_new_on", [device]) if jit_value_type(&device.ty) => {
            resident_safe_expr(device, callees)
        }
        ("stream_sync" | "stream_show", [stream]) if jit_value_type(&stream.ty) => {
            resident_safe_expr(stream, callees)
        }
        ("transfer", [tensor, device])
            if tensor.ty.is_compute_tensor_family() && jit_value_type(&device.ty) =>
        {
            args.iter().all(|arg| resident_safe_expr(arg, callees))
        }
        ("transfer_show", [tensor]) if tensor.ty.is_compute_tensor_family() => {
            resident_safe_expr(tensor, callees)
        }
        ("kernel_bounds_ok", [shape, indices])
            if jit_list_int_type(&shape.ty) && jit_list_int_type(&indices.ty) =>
        {
            args.iter().all(|arg| resident_safe_expr(arg, callees))
        }
        ("to_sparse", [tensor]) if tensor.ty.is_compute_tensor_family() => {
            resident_safe_expr(tensor, callees)
        }
        ("sparse_nnz" | "sparse_show", [sparse]) if jit_value_type(&sparse.ty) => {
            resident_safe_expr(sparse, callees)
        }
        ("sparse_mv", [sparse, vector])
            if jit_value_type(&sparse.ty) && vector.ty.is_compute_tensor_family() =>
        {
            args.iter().all(|arg| resident_safe_expr(arg, callees))
        }
        ("add" | "mul" | "sub" | "div" | "maximum" | "minimum" | "matmul", [left, right])
            if left.ty.is_compute_tensor_family() && right.ty.is_compute_tensor_family() =>
        {
            args.iter().all(|arg| resident_safe_expr(arg, callees))
        }
        ("negate" | "abs" | "exp" | "log" | "sqrt", [tensor])
            if tensor.ty.is_compute_tensor_family() =>
        {
            resident_safe_expr(tensor, callees)
        }
        ("sum_axis", [tensor, axis])
            if tensor.ty.is_compute_tensor_family() && intish_ty(&axis.ty) =>
        {
            args.iter().all(|arg| resident_safe_expr(arg, callees))
        }
        ("mse_loss", [left, right])
            if left.ty.is_compute_tensor_family() && right.ty.is_compute_tensor_family() =>
        {
            args.iter().all(|arg| resident_safe_expr(arg, callees))
        }
        ("sgd_step", [param, grad, learning_rate])
            if param.ty.is_compute_tensor_family()
                && grad.ty.is_compute_tensor_family()
                && matches!(
                    erase_runtime_qualifiers(&learning_rate.ty),
                    Type::Float | Type::Float32
                ) =>
        {
            args.iter().all(|arg| resident_safe_expr(arg, callees))
        }
        ("serialize", [tensor]) if tensor.ty.is_compute_tensor_family() => {
            resident_safe_expr(tensor, callees)
        }
        ("deserialize", [payload]) if matches!(erase_runtime_qualifiers(&payload.ty), Type::String) => {
            resident_safe_expr(payload, callees)
        }
        ("matmul_f32_tile", [left, right])
            if left.ty.is_compute_tensor_family() && right.ty.is_compute_tensor_family() =>
        {
            args.iter().all(|arg| resident_safe_expr(arg, callees))
        }
        ("get", [tensor, indices])
            if tensor.ty.is_compute_tensor_family() && jit_list_int_type(&indices.ty) =>
        {
            args.iter().all(|arg| resident_safe_expr(arg, callees))
        }
        ("set", [tensor, indices, value])
            if tensor.ty.is_compute_tensor_family()
                && jit_list_int_type(&indices.ty)
                && matches!(erase_runtime_qualifiers(&value.ty), Type::Float | Type::Float32) =>
        {
            args.iter().all(|arg| resident_safe_expr(arg, callees))
        }
        ("shape", [tensor]) if tensor.ty.is_compute_tensor_family() => {
            resident_safe_expr(tensor, callees)
        }
        ("rank" | "numel" | "device" | "placement", [tensor])
            if tensor.ty.is_compute_tensor_family() =>
        {
            resident_safe_expr(tensor, callees)
        }
        ("profile_f32_strict" | "profile_show", []) => true,
        ("to_list", [tensor]) if tensor.ty.is_compute_tensor_family() => {
            resident_safe_expr(tensor, callees)
        }
        _ => false,
    }
}

fn resident_safe_compute_gradient_result(ty: &Type) -> bool {
    ty.is_compute_tensor_family()
        || matches!(ty, Type::Tuple(fields) if fields
            .iter()
            .all(|(_, field)| resident_safe_compute_gradient_result(field)))
}

fn resident_safe_compute_result(method: &str, args: &[TExpr], result_ty: &Type) -> bool {
    let result = if args.len() == 2 {
        let Type::Fn { ret: Some(ret), .. } = result_ty else {
            return false;
        };
        ret.as_ref()
    } else {
        result_ty
    };
    match method {
        "gradient" => resident_safe_compute_gradient_result(result),
        "value_and_gradient" => matches!(result, Type::Tuple(fields) if fields.len() == 2
            && fields[0].1.is_compute_tensor_family()
            && resident_safe_compute_gradient_result(fields[1].1.as_ref())),
        "jvp" => matches!(result, Type::Tuple(fields) if fields.len() == 2
            && fields.iter().all(|(_, field)| field.is_compute_tensor_family())),
        "vjp" => matches!(result, Type::Apply { name, args } if name == "VjpRun"
            && args.len() == 1
            && resident_safe_compute_gradient_result(&args[0])),
        _ => false,
    }
}

pub(crate) fn is_packed_process_signal(expr: &TExpr) -> bool {
    // Sema inserts `Expr::Copy` for this non-scalar field when it is used as
    // an owning pattern subject. TIR represents that copy as `Clone`; it is a
    // bitwise copy because `lower_clone` copies a packed `Option` word
    // (`Type::Option(_)` row). Keep the unwrap exact so no other cloned field
    // becomes a packed Option carrier.
    let field = match &expr.kind {
        TExprKind::Clone(inner) => inner,
        _ => expr,
    };
    match &field.kind {
        TExprKind::Field {
            recv,
            field,
            boxed: false,
        } if field == "signal"
            && matches!(&recv.ty, Type::Named(name) if name == "ProcessResult" || name == "ProcessReceipt") => true,
        _ => false,
    }
}

/// The one comptime-value table (I8).
///
/// `slot` is the type the value has to land in, when a caller knows it. A
/// `TExprKind::CtLit` node carries `expr.ty` and has already passed
/// `jit_value_type`, so its ABI is pinned; `None` means the value sits inside
/// another comptime value, where `lower_ct_value` picks the ABI from the value's
/// own `jet_type()` instead. Only the arms where that choice differs read `slot`,
/// and each one says why.
///
/// This was two tables: an inline `TExprKind::CtLit` or-pattern that admitted
/// `BigInt` but neither `Float` nor `Unit`, and this helper, which admitted
/// `Float` and `Unit` but not `BigInt`. Same fact, two answers.
fn resident_safe_ct_value(value: &jet_foundation::AST::CtValue, slot: Option<&Type>) -> bool {
    use jet_foundation::AST::{CtKey, CtReport, CtValue};
    match value {
        CtValue::Int(_) | CtValue::Bool(_) | CtValue::Char(_) | CtValue::Str(_) => true,
        // `CtValue::Unit` lowers to the same zero word as `TExprKind::Unit`.
        CtValue::Unit => true,
        // A comptime `Int` outside the small range lowers through
        // `heap.int_from_str`, which packs the same tagged-Int word
        // `TExprKind::IntLit` gets from `int_from_i64` (lower_ctx.rs). That word is
        // an `Int` wherever an `Int` goes, so no slot changes the answer.
        CtValue::BigInt(_) => true,
        // `CtValue::Float` lowers with a bare `f64const` (lower_ctx.rs
        // `lower_ct_value`), which does not round through f32 the way
        // `TExprKind::FloatLit` does for a `Float32` slot. Nested, no slot claims
        // f32: the record setter and `list_push_f64` follow the value's own
        // `CtFloat` width.
        CtValue::Float(_) => slot.is_none_or(|ty| matches!(ty, Type::Float)),
        // A comptime Map literal uses the same host adapter as a runtime
        // literal. Composite keys lower as records; scalar strings retain the
        // established string ABI. The slot pins which key carrier is valid.
        CtValue::Map(entries) => slot.is_some_and(|ty| {
            jit_map_resident_type(ty)
                && match ty {
                    Type::Map { key: key_ty, value: value_ty, .. } => {
                        jit_value_type(value_ty)
                            && entries.iter().all(|(key, entry)| {
                                (if jit_map_composite_key_type(key_ty) {
                                    matches!(key, CtKey::Tuple(_) | CtKey::Struct { .. })
                                } else {
                                    matches!(key, CtKey::Str(_))
                                })
                                    && resident_safe_ct_value(entry, Some(value_ty.as_ref()))
                            })
                    }
                    _ => false,
                }
        }),
        // With a slot, the list type itself passed `jit_value_type`, which pins
        // the element ABI, and the `CtValue::List` arm pushes every element
        // through the same `list_push`/`list_push_f64` a runtime list literal
        // uses. Nested, nothing pinned it, so each element must stand alone.
        CtValue::List(items) => match slot {
            Some(_) => true,
            None => items
                .iter()
                .all(|item| resident_safe_ct_value(item, None)),
        },
        // Field slots come from the struct layout, which `lower_ct_struct` reads
        // for itself; safety has no layout access, so each field value stands
        // alone here whether or not the struct's own slot is known.
        CtValue::Struct { fields, .. } => fields
            .iter()
            .all(|(_, field)| resident_safe_ct_value(field, None)),
        // Anonymous-union field payloads lower as `CtValue::Enum` (#1444
        // `Box.{value: 9}`), which is the nested case this arm has always been
        // about. It does not extend to a `CtLit` whose own slot is an enum:
        // `lower_ct_enum` has no `pack_datatree_enum` arm, so for
        // DataTree/JSON/TOML/YAML/CSV it would build a different carrier than
        // `TExprKind::EnumLit` builds, and safety may not keep a second copy of
        // that type list to say so (I8).
        CtValue::Enum { args, .. } => {
            slot.is_none() && args.iter().all(|(_, arg)| resident_safe_ct_value(arg, None))
        }
        // D-FAIL-CARRIER1=A: one `Present` carries both outcome views, so the slot
        // picks which carrier `lower_ct_value` builds — the packed Option word
        // (`pack_option_payload`, absent = 0) or the result arena
        // (`lower_ct_result`, the same `result_new_*` handle `TExprKind::Ok`
        // builds). `Option<IntN>` is excluded: its present side is arena-carried
        // while a clean report still lowers to the packed zero, so the two sides
        // of one option would disagree (lower_ctx.rs `option_present_flag`).
        CtValue::Present(inner) => match slot {
            Some(Type::Option(payload)) => {
                !matches!(payload.as_ref(), Type::IntN { .. })
                    && resident_safe_ct_value(inner, Some(payload.as_ref()))
            }
            Some(Type::Result { ok, .. }) => resident_safe_ct_value(inner, Some(ok.as_ref())),
            Some(_) => false,
            None => resident_safe_ct_value(inner, None),
        },
        CtValue::Failed(CtReport::Clean(_)) => match slot {
            Some(Type::Option(payload)) => !matches!(payload.as_ref(), Type::IntN { .. }),
            Some(_) => false,
            None => true,
        },
        CtValue::Failed(CtReport::Told(inner)) => match slot {
            Some(Type::Result { err, .. }) => resident_safe_ct_value(inner, Some(err.as_ref())),
            Some(_) => false,
            None => resident_safe_ct_value(inner, None),
        },
        _ => false,
    }
}

fn jit_scalar_type(ty: &Type) -> bool {
    jit_value_type(ty)
}

fn erase_runtime_qualifiers(mut ty: &Type) -> &Type {
    while let Type::Tagged { inner, .. } = ty {
        ty = inner;
    }
    ty
}

fn intish_ty(ty: &Type) -> bool {
    matches!(
        erase_runtime_qualifiers(ty),
        Type::Int | Type::IntN { .. } | Type::InlineRange { .. }
    )
}

/// `LowerCtx::lower_binary`'s two Option rows, stated once for both copies of
/// the Binary gate.
///
/// Equality is presence-then-payload for any payload
/// (`LowerCtx::lower_option_eq`). Ordering is Rust's `Option: PartialOrd`
/// (`LowerCtx::lower_option_order`, the law AOT gets from the plain Rust
/// operator): presence alone decides unless both sides are present, and then
/// `lower_value_order` decides on the payload — so ordering is admitted for
/// exactly the payload types that function has a row for. Arithmetic on an
/// Option has no row at all, so it stays out rather than reaching a machine op
/// on the carrier word.
fn resident_safe_option_binary(op: &BinOp, lhs: &Type, rhs: &Type) -> bool {
    fn payload_orderable(ty: &Type) -> bool {
        let Type::Option(inner) = erase_runtime_qualifiers(ty) else {
            // The non-Option side of a mixed pair: `lower_option_order` reads
            // the payload type off the Option operand.
            return true;
        };
        matches!(
            erase_runtime_qualifiers(inner.as_ref()),
            Type::Int
                | Type::IntN { .. }
                | Type::Char
                | Type::Bool
                | Type::Float
                | Type::Float32
                | Type::String
                | Type::List(_)
                | Type::FixedList { .. }
        )
    }
    match op {
        BinOp::Eq | BinOp::Ne => true,
        BinOp::Lt | BinOp::Gt | BinOp::Le | BinOp::Ge => {
            payload_orderable(lhs) && payload_orderable(rhs)
        }
        _ => false,
    }
}

/// The gate is a SECOND copy of tables `lower_ctx.rs` already owns, and every
/// refusal in this file that turned out to be wrong was that copy lagging the
/// lowering rather than the lowering missing a row. These checks fail when the
/// two disagree, and name the row.
///
/// What is pinned, and why only this much: a table is checkable here when both
/// sides reduce to a pure predicate over an enumerable domain. `BinOp` is a
/// closed enum (`op_name` below has no `_` arm, so a new operator stops the
/// build until someone decides its Option row), and `lower_value_order`'s
/// operand table is `LowerCtx::value_order_supported`, which the lowering
/// refuses through.
///
/// `TBuiltinOp::Unzip` is not pinned by a test on THIS side because it no
/// longer needs one: its receiver/column table is
/// `LowerCtx::unzip_column_kinds`, which BOTH the gate arm and the
/// `TBuiltinOp::Unzip` lowering read (the `service_core_arity` treatment — the
/// second copy is deleted, not tested against the first). Every table that can
/// take that treatment should, and a test is the fallback for the ones that
/// cannot.
///
/// What that lift moved rather than removed: the receiver table is now one
/// copy, but the host still has to READ each column word back in the kind it
/// was WRITTEN in, and those two ladders live on either side of the arena. That
/// pair is the one that shipped two empty lists, it is total over
/// `JitZipValueKind`, and it is pinned in `Collections.rs`'s
/// `unzip_column_round_trip` — every kind, optional and not, at zero, one and
/// three rows.
///
/// The rest of the `THandleOp` and `TBuiltinOp` method tables are still two
/// copies and still unpinned: their lowering halves are `&mut self` emitters
/// that need a live `FunctionBuilder`, a Cranelift module and a host table, so
/// there is no pure set to compare against until each dispatch is lifted into a
/// table of its own, one op family at a time.
///
/// Known residual, deliberately NOT closed here: the gate reaches the payload
/// through `erase_runtime_qualifiers` (Tagged only) while the lowering reaches
/// it through `LowerCtx::erase_distinct_ty`, which also peels `Quantity`, Core
/// type aliases and `distinct` bases off a `Named`. So `Option<Weight>` for a
/// `distinct Weight = Int` is ordered by the lowering and refused by the gate.
/// That direction is a deopt, never a wrong answer, and closing it needs the
/// `JitMeta` tables the gate has no handle on at planning time — so it is a
/// named gap, not a silent one.
#[cfg(test)]
mod gate_follows_lowering {
    use super::*;
    use jet_foundation::AST::Measure;

    /// Every `BinOp`, with no `_` arm: a new operator must fail to compile here
    /// rather than quietly join or miss the Option table below.
    fn op_name(op: BinOp) -> &'static str {
        match op {
            BinOp::Add => "Add",
            BinOp::Sub => "Sub",
            BinOp::Mul => "Mul",
            BinOp::Div => "Div",
            BinOp::FloorDiv => "FloorDiv",
            BinOp::Mod => "Mod",
            BinOp::Rem => "Rem",
            BinOp::Pow => "Pow",
            BinOp::BitAnd => "BitAnd",
            BinOp::BitOr => "BitOr",
            BinOp::BitXor => "BitXor",
            BinOp::Shl => "Shl",
            BinOp::Shr => "Shr",
            BinOp::Eq => "Eq",
            BinOp::Ne => "Ne",
            BinOp::Lt => "Lt",
            BinOp::Gt => "Gt",
            BinOp::Le => "Le",
            BinOp::Ge => "Ge",
            BinOp::Compare => "Compare",
            BinOp::And => "And",
            BinOp::Or => "Or",
        }
    }

    const EVERY_BIN_OP: [BinOp; 22] = [
        BinOp::Add,
        BinOp::Sub,
        BinOp::Mul,
        BinOp::Div,
        BinOp::FloorDiv,
        BinOp::Mod,
        BinOp::Rem,
        BinOp::Pow,
        BinOp::BitAnd,
        BinOp::BitOr,
        BinOp::BitXor,
        BinOp::Shl,
        BinOp::Shr,
        BinOp::Eq,
        BinOp::Ne,
        BinOp::Lt,
        BinOp::Gt,
        BinOp::Le,
        BinOp::Ge,
        BinOp::Compare,
        BinOp::And,
        BinOp::Or,
    ];

    /// `lower_binary`'s two `Type::Option` arms, transcribed: `Eq | Ne` reach
    /// `lower_option_eq`, `Lt | Gt | Le | Ge` reach `lower_option_order`, and no
    /// other operator has an arm for an Option operand.
    const LOWERED_OPTION_OPS: [BinOp; 6] = [
        BinOp::Eq,
        BinOp::Ne,
        BinOp::Lt,
        BinOp::Gt,
        BinOp::Le,
        BinOp::Ge,
    ];

    #[test]
    fn option_binary_admits_exactly_the_operators_lower_binary_dispatches() {
        let operand = Type::Option(Box::new(Type::Int));
        for op in EVERY_BIN_OP {
            let admitted = resident_safe_option_binary(&op, &operand, &operand);
            let lowered = LOWERED_OPTION_OPS.contains(&op);
            assert_eq!(
                admitted, lowered,
                "`Int? {} Int?`: the residency gate admits={admitted} but \
                 lower_binary has an Option arm={lowered}. One of the two is \
                 behind; fix whichever is.",
                op_name(op)
            );
        }
    }

    /// Every payload shape a JIT operand can carry, so the comparison below is
    /// not vacuous on either side of the boundary.
    fn candidate_payload_types() -> Vec<Type> {
        vec![
            Type::Int,
            Type::IntN {
                signed: true,
                bits: 32,
            },
            Type::IntN {
                signed: false,
                bits: 8,
            },
            Type::Char,
            Type::Bool,
            Type::Float,
            Type::Float32,
            Type::String,
            Type::List(Box::new(Type::Int)),
            Type::List(Box::new(Type::String)),
            Type::FixedList {
                elem: Box::new(Type::Int),
                len: Measure::Literal {
                    kind: "length".to_string(),
                    value: 4,
                },
            },
            Type::Named("Date".to_string()),
            Type::Option(Box::new(Type::Int)),
            Type::Map {
                key: Box::new(Type::String),
                key_span: None,
                value: Box::new(Type::Int),
            },
            Type::Tuple(Vec::new()),
            Type::Union(vec![Type::Int, Type::String]),
            Type::Apply {
                name: "Set".to_string(),
                args: vec![Type::Int],
            },
            Type::Result {
                ok: Box::new(Type::Int),
                err: Box::new(Type::String),
            },
            Type::TraitObject(vec!["Shape".to_string()]),
        ]
    }

    #[test]
    fn option_ordering_admits_exactly_the_payloads_lower_value_order_lowers() {
        for ty in candidate_payload_types() {
            let operand = Type::Option(Box::new(ty.clone()));
            let admitted = resident_safe_option_binary(&BinOp::Lt, &operand, &operand);
            let lowered = crate::lower_ctx::LowerCtx::value_order_supported(&ty);
            assert_eq!(
                admitted, lowered,
                "`{ty:?}?` under `<`: the residency gate admits={admitted} but \
                 LowerCtx::value_order_supported={lowered}. This is the #2036 \
                 shape: one side grew a row and the other did not."
            );
        }
    }

    /// `LowerCtx::lower_expr` emits ONE instruction for both zero-word
    /// literals — `TExprKind::Absent` and `TExprKind::Unit` are each
    /// `iconst(I64, 0)` — so the gate may not answer for one and refuse the
    /// other. It refused `Unit`, which fell into `resident_safe_expr_recursive`'s
    /// `_ => false`, and that refused every fallible-void `fn run` spelling an
    /// early bare `return`: D-FAIL-EXIT1 makes such a `run` return `!`,
    /// and TIR lowers the bare `return` to `Return(Some(Ok(Unit)))`
    /// (`lower/statements.rs`). `examples/features/io/watcher.jet:15` was the
    /// corpus case.
    #[test]
    fn the_two_zero_word_literals_are_admitted_together() {
        let callees: HashSet<String> = HashSet::new();
        let unit = TExpr {
            ty: Type::Named("Unit".to_string()),
            kind: TExprKind::Unit,
        };
        let absent = TExpr {
            ty: Type::Option(Box::new(Type::Int)),
            kind: TExprKind::Absent,
        };
        assert_eq!(
            resident_safe_expr(&unit, &callees),
            resident_safe_expr(&absent, &callees),
            "TExprKind::Unit and TExprKind::Absent both lower to iconst(I64, 0); \
             the gate must not admit one and refuse the other"
        );
        assert!(
            resident_safe_expr(&unit, &callees),
            "TExprKind::Unit has a lowering row (iconst(I64, 0)), so the gate \
             must admit it"
        );
    }

    fn int_local(name: &str) -> TExpr {
        TExpr {
            ty: Type::Int,
            kind: TExprKind::Local(TIR::TLocal::user(name)),
        }
    }

    fn ptr_ty(inner: Type) -> Type {
        Type::Apply {
            name: jet_foundation::Syntax::TYPE_PTR.to_string(),
            args: vec![inner],
        }
    }

    /// Every `*x` operand shape the corpus writes inside `#Unsafe`, plus the
    /// ones the lowering has to decline.
    fn candidate_raw_operands() -> Vec<TExpr> {
        let inner_raw_of = TExpr {
            ty: ptr_ty(Type::Int),
            kind: TExprKind::RawOf(Box::new(int_local("cell"))),
        };
        vec![
            // `*cell` on a live local — `examples/features/memory/rawptr.jet`.
            int_local("cell"),
            // The `*Int.{*cell}` outer cast, whose operand is already a pointer.
            inner_raw_of,
            // A borrow of a local is still that local's place.
            TExpr {
                ty: Type::Int,
                kind: TExprKind::Borrow {
                    place: Box::new(int_local("cell")),
                    mutable: false,
                },
            },
            // Not a place at all: `*arena.alloc(7)`
            // (`examples/features/memory/unsafe_sentries.jet`) reduced to the
            // one fact that decides it.
            TExpr {
                ty: Type::Int,
                kind: TExprKind::Call {
                    name: "alloc".to_string(),
                    args: Vec::new(),
                    type_args: Vec::new(),
                },
            },
            // A place whose payload has no machine slot.
            TExpr {
                ty: Type::Named("Unit".to_string()),
                kind: TExprKind::Local(TIR::TLocal::user("nothing")),
            },
        ]
    }

    /// D-CAP9 / D-MEM-SENTRY1: the gate admits a raw pointer exactly when
    /// `LowerCtx`'s `TExprKind::RawOf` arm can mint one — a `Ptr<_>` operand it
    /// passes through, or a bare local place with a machine-sized payload. The
    /// arm answered a flat `false` for every operand while the lowering had
    /// covered the bare-local slot all along, and the non-place branch minted a
    /// pointer to a COPY, which is the shape that would answer 7 instead of
    /// R0802 for `*arena.alloc(7)` after `arena.reset()`.
    #[test]
    fn raw_of_admits_exactly_the_operands_the_lowering_can_mint() {
        // Transcribed from `LowerCtx`'s `TExprKind::RawOf` arm, in its order:
        // the `Ptr<_>` passthrough, a bare-local place with a machine payload,
        // then decline. A fixed verdict per shape, so widening either side has
        // to come here and say so.
        let expected = [true, true, true, false, false];
        let operands = candidate_raw_operands();
        assert_eq!(operands.len(), expected.len());
        for (operand, want) in operands.iter().zip(expected) {
            let admitted = resident_safe_raw_of(operand);
            assert_eq!(
                admitted,
                want,
                "`*<{}>` of type {:?}: expected the residency gate to answer \
                 {want}, matching LowerCtx's RawOf arm",
                expr_kind_tag(operand),
                operand.ty
            );
            if admitted {
                let passthrough = matches!(&operand.ty, Type::Apply { name, args }
                    if name == jet_foundation::Syntax::TYPE_PTR && args.len() == 1);
                assert!(
                    passthrough
                        || crate::lower_ctx::LowerCtx::raw_place_local(operand).is_some(),
                    "the gate admitted `*<{}>`, which has no storage of its own. \
                     LowerCtx's RawOf arm declines it, so this is a hard failure \
                     in the strict tier — and a pointer to a COPY if that branch \
                     is ever restored",
                    expr_kind_tag(operand)
                );
            }
        }
    }

    /// The pointee ABI is the whole `p.*` question: `LowerCtx`'s
    /// `TExprKind::Deref` arm loads `meta.clif_ty(&expr.ty)`, so a result with no
    /// machine slot has nothing to load into.
    #[test]
    fn deref_admits_exactly_the_results_with_a_machine_slot() {
        for (ty, want) in [
            (Type::Int, true),
            (Type::Bool, true),
            (Type::Float, true),
            (Type::Named("Unit".to_string()), false),
        ] {
            assert_eq!(
                resident_safe_raw_deref(&ty),
                want,
                "`p.*` into {ty:?}: expected the residency gate to answer {want}"
            );
        }
    }

    fn trait_arg(ty: Type) -> TCallArg {
        TCallArg {
            value: TExpr {
                ty,
                kind: TExprKind::Local(TIR::TLocal::user("one")),
            },
            template_items: None,
            borrow: false,
            mut_borrow: false,
            clone: false,
            arc_clone: false,
            fn_coerce: None,
            widen_to_vec: false,
            widen_to_union: None,
            box_as_trait: Some("Shape".to_string()),
        }
    }

    /// S48: a concrete entering a trait value slot is boxed by
    /// `LowerCtx::lower_trait_object_box`, whose type id is
    /// `JitMeta::struct_type_id` — a position in the `struct_fields` table. A
    /// nominal name can have one; a tuple never does. The gate refused ALL of
    /// them, so `print_area(one)` deopted `examples/features/types/traits.jet`.
    #[test]
    fn a_trait_slot_admits_the_nominal_concretes_the_box_can_identify() {
        assert!(resident_safe_trait_arg(&trait_arg(Type::Named(
            "Circle".to_string()
        ))));
        assert!(!resident_safe_trait_arg(&trait_arg(Type::Tuple(Vec::new()))));
        assert!(!resident_safe_trait_arg(&trait_arg(Type::Int)));
    }

    /// I8: the crypto pair table lives in `LowerCtx::crypto_core_arity` and the
    /// gate reads it, so these rows cannot drift apart again. They are the eight
    /// the deleted hand-copy was missing —
    /// `examples/features/crypto/random_api_split.jet` used two of them.
    #[test]
    fn the_crypto_gate_admits_every_pair_the_lowering_has_a_host_for() {
        for (module, method, arity) in [
            ("core.crypto", "hkdf_sha256", 4usize),
            ("core.crypto", "constant_time_equal", 2),
            ("core.crypto", "constant_time_equal_bytes", 2),
            ("core.crypto", "__secret_from_bytes", 1),
            ("core.crypto", "__password_text", 1),
            ("core.crypto", "x25519_public", 1),
            ("core.crypto", "x25519_shared", 2),
            ("core.crypto", "file_seal", 3),
            ("core.crypto.expert", "hkdf_sha256_raw", 4),
        ] {
            assert_eq!(
                crate::lower_ctx::LowerCtx::crypto_core_arity(module, method),
                Some(arity),
                "{module}.{method} has a resident host row, so the one arity \
                 table must name it"
            );
        }
        assert_eq!(
            crate::lower_ctx::LowerCtx::crypto_core_arity("core.crypto", "nope"),
            None
        );
    }

    /// A spawn site TIR already assigned is read off the node, so two
    /// references to one index are ONE table entry. The fenced-name fan
    /// `@[ t1..t8 ]@ :: task transfer(…)` in
    /// `examples/features/memory/shared_transact.jet` is eight such references
    /// to entry 0, and comparing a raw expression count against the table size
    /// reported `spawn site count 8 != lambda count 1`.
    #[test]
    fn repeated_references_to_one_spawn_site_are_one_table_entry() {
        let mut tally = SpawnSiteTally::default();
        for _ in 0..8 {
            tally.assigned_site(0);
        }
        assert_eq!(tally.total(), 1);
        tally.assigned_site(1);
        assert_eq!(tally.total(), 2);
        tally.cursor_slot();
        tally.cursor_slot();
        assert_eq!(
            tally.total(),
            4,
            "a cursor-derived callback consumes a fresh slot per occurrence"
        );
    }
}

/// `Signal/Derived/Computed.get()` — TIR often erases the Apply payload to
/// `Named("Unit")` inside closures. Overflow arithmetic still uses the Int
/// host path for Int signals (`n.get() * 2`).
fn reactive_get_intish(expr: &TExpr) -> bool {
    matches!(
        &expr.kind,
        TExprKind::HandleMethod {
            op: THandleOp::ReactiveGet,
            ..
        }
    )
}

fn jit_bag_raw_key_candidate(ty: &Type) -> bool {
    match ty {
        Type::Tagged { inner, .. } => jit_bag_raw_key_candidate(inner),
        Type::Int
            | Type::IntN { .. }
            | Type::InlineRange { .. }
            | Type::Bool
            | Type::Char
            | Type::Named(_) => true,
        _ => false,
    }
}

pub(crate) fn jit_list_int_type(ty: &Type) -> bool {
    // IntN (U8/…) shares the i64 list ABI — bytes / write_at / random.bytes.
    matches!(
        ty,
        Type::List(inner) if intish_ty(inner)
    ) || matches!(
        ty,
        Type::FixedList { elem, .. } if intish_ty(elem)
    )
}

pub(crate) fn jit_list_float_type(ty: &Type) -> bool {
    matches!(
        ty,
        Type::List(inner) if matches!(inner.as_ref(), Type::Float | Type::Float32)
    ) || matches!(
        ty,
        Type::FixedList { elem, .. } if matches!(elem.as_ref(), Type::Float | Type::Float32)
    )
}

fn jit_float_view_type(ty: &Type) -> bool {
    matches!(
        ty,
        Type::Apply { name, args }
            if matches!(name.as_str(), "View" | "ViewMut" | "ComputeViewMut")
                && args.len() == 1
                && matches!(&args[0], Type::Float)
    )
}

fn jit_float_view_mut_type(ty: &Type) -> bool {
    matches!(
        ty,
        Type::Apply { name, args }
            if matches!(name.as_str(), "ViewMut" | "ComputeViewMut")
                && args.len() == 1
                && matches!(&args[0], Type::Float)
    )
}

pub(crate) fn jit_list_string_type(ty: &Type) -> bool {
    matches!(ty, Type::List(inner) if matches!(inner.as_ref(), Type::String))
        || matches!(ty, Type::FixedList { elem, .. } if matches!(elem.as_ref(), Type::String))
}

pub(crate) fn jit_list_native_type(ty: &Type) -> bool {
    jit_list_int_type(ty)
        || jit_list_float_type(ty)
        || jit_list_string_type(ty)
        || jit_list_option_type(ty)
}

/// `[T?]` for optional scalars (series missing counts, etc.).
pub(crate) fn jit_list_option_type(ty: &Type) -> bool {
    matches!(ty, Type::List(inner) if jit_optional_scalar_type(inner))
}

fn jit_list_intn_type(ty: &Type) -> bool {
    matches!(
        ty,
        Type::List(inner) | Type::FixedList { elem: inner, .. }
            if matches!(inner.as_ref(), Type::IntN { .. })
    )
}

/// `[[Int]]` / `Iter<[Int]>` — flat_map identity receivers in iter_adapters.
pub(crate) fn jit_list_of_int_list_type(ty: &Type) -> bool {
    matches!(ty, Type::List(inner) if jit_list_int_type(inner))
        || matches!(
            ty,
            Type::Apply { name, args }
                if name == jet_foundation::Syntax::TYPE_ITER
                    && args.len() == 1
                    && jit_list_int_type(&args[0])
        )
}

pub(crate) fn jit_list_iter_elem_type(ty: &Type) -> Option<Type> {
    match ty {
        Type::List(inner) | Type::FixedList { elem: inner, .. }
            if matches!(inner.as_ref(), Type::InlineRange { .. }) =>
        {
            Some(Type::Int)
        }
        Type::List(inner) | Type::FixedList { elem: inner, .. }
            if matches!(
                inner.as_ref(),
                Type::Int
                    | Type::IntN { .. }
                    | Type::Float
                    | Type::Float32
                    | Type::String
                    | Type::Char
            ) =>
        {
            Some(inner.as_ref().clone())
        }
        // A list returned by `get_disjoint_write` contains opaque mutable-window
        // handles. Sema makes iteration lending-only, so the JIT may load one
        // handle for the current turn without granting an escaping alias.
        Type::List(inner) | Type::FixedList { elem: inner, .. }
            if matches!(
                inner.as_ref(),
                Type::Apply { name, args }
                    if matches!(name.as_str(), "ViewMut" | "ComputeViewMut")
                        && args.len() == 1
                        && (matches!(
                            &args[0],
                            Type::Int | Type::Float | Type::String | Type::Char
                        ) || record_type_key(&args[0]).is_some())
            ) =>
        {
            Some(inner.as_ref().clone())
        }
        // Nested list rows (`[[String]]` CSV, etc.) — outer iterates i64 handles.
        Type::List(inner) | Type::FixedList { elem: inner, .. }
            if jit_list_native_type(inner) =>
        {
            Some(inner.as_ref().clone())
        }
        // `List<Map<String, V>>` (db query rows) — map handles are i64.
        Type::List(inner) | Type::FixedList { elem: inner, .. }
            if matches!(
                inner.as_ref(),
                Type::Map { key, .. } if matches!(key.as_ref(), Type::String)
            ) =>
        {
            Some(inner.as_ref().clone())
        }
        // JIT ABI: `Iter<T>` / `View<T>` / `ViewMut<T>` producers materialize list
        // handles — scalar elems share list for-in / join / len; record elems too.
        Type::Apply { name, args }
            if (name == jet_foundation::Syntax::TYPE_ITER
                || matches!(name.as_str(), "View" | "ViewMut" | "ComputeViewMut"))
                && args.len() == 1
                && jit_list_native_type(&args[0]) =>
        {
            Some(args[0].clone())
        }
        // Iterators produced by chunks/windows carry list-valued elements.
        Type::Apply { name, args }
            if (name == jet_foundation::Syntax::TYPE_ITER
                || matches!(name.as_str(), "View" | "ViewMut" | "ComputeViewMut"))
                && args.len() == 1
                && (matches!(
                    &args[0],
                    Type::Int | Type::Float | Type::String | Type::Char
                ) || record_type_key(&args[0]).is_some()) =>
        {
            Some(args[0].clone())
        }
        _ => None,
    }
}

fn jit_zip_elem_type(ty: &Type) -> bool {
    matches!(
        ty,
        Type::Int
            | Type::IntN { .. }
            | Type::Float
            | Type::Float32
            | Type::Bool
            | Type::Char
            | Type::String
    ) || jit_optional_scalar_type(ty)
        || jit_value_type(ty)
}

fn jit_zip_sequence_elem_type(ty: &Type) -> Option<Type> {
    jit_list_iter_elem_type(ty).or_else(|| jit_closure_elem_type(ty))
}

fn jit_zip_field_type(ty: &Type) -> bool {
    match ty {
        Type::Option(inner) => jit_zip_elem_type(inner),
        _ => jit_zip_elem_type(ty),
    }
}

/// Closure adapter receivers: scalar list/Iter/View elems, or user struct handles.
pub(crate) fn jit_closure_elem_type(ty: &Type) -> Option<Type> {
    if let Some(elem) = jit_list_iter_elem_type(ty) {
        return Some(elem);
    }
    match ty {
        Type::List(inner) | Type::FixedList { elem: inner, .. }
            if record_type_key(inner).is_some() =>
        {
            Some(inner.as_ref().clone())
        }
        Type::Apply { name, args }
            if (name == jet_foundation::Syntax::TYPE_ITER
                || matches!(name.as_str(), "View" | "ViewMut" | "ComputeViewMut"))
                && args.len() == 1
                && record_type_key(&args[0]).is_some() =>
        {
            Some(args[0].clone())
        }
        _ => None,
    }
}

/// The element a single-index closure loop can bind straight off `list_get`:
/// one i64 the callback reads as an `Int`, a `String` handle, or a record
/// handle. `Float` stays out on purpose — it needs `list_get_f64`, a different
/// host fn — and so does `Set`, which `jit_closure_elem_type_for` adds: every
/// caller of this helper walks the receiver BY INDEX, and a hash `Set`
/// publishes no position (D-SET-DECLINE1=C). Paired with
/// `LowerCtx::lower_iter_position`, `lower_iter_take_skip_while`,
/// `lower_iter_min_max_by` and `lower_iter_count_by`, which all bind the
/// element at this type instead of assuming `Int`.
pub(crate) fn jit_closure_loop_elem(ty: &Type) -> Option<Type> {
    jit_closure_elem_type(ty)
        .filter(|elem| matches!(elem, Type::Int | Type::String | Type::Named(_)))
}

pub(crate) fn jit_closure_elem_type_for(ty: &Type) -> Option<Type> {
    jit_closure_elem_type(ty).or_else(|| match ty {
        Type::Apply { name, args }
            if name == jet_foundation::Syntax::TYPE_SET && args.len() == 1 =>
        {
            Some(args[0].clone())
        }
        // A `[Shape]` element is one i64 handle like every other list element —
        // `jit_list_record_type` already treats it as a record row, and dynamic
        // dispatch inside the callback lowers through `lower_trait_object_method`.
        // Only the arms that name `TraitObject` in their element filter admit it.
        Type::List(inner) | Type::FixedList { elem: inner, .. }
            if matches!(inner.as_ref(), Type::TraitObject(traits) if !traits.is_empty()) =>
        {
            Some(inner.as_ref().clone())
        }
        _ => None,
    })
}

/// `[T?E]` / `Iter<T?E>` with JIT-representable payloads.
pub(crate) fn jit_result_list_elem(ty: &Type) -> Option<(Type, Type)> {
    let elem = match ty {
        Type::List(inner) | Type::FixedList { elem: inner, .. } => inner.as_ref(),
        Type::Apply { name, args }
            if name == jet_foundation::Syntax::TYPE_ITER && args.len() == 1 =>
        {
            &args[0]
        }
        _ => return None,
    };
    match elem {
        Type::Result { ok, err }
            if jit_result_payload_type(ok) && jit_result_payload_type(err) =>
        {
            Some((ok.as_ref().clone(), err.as_ref().clone()))
        }
        _ => None,
    }
}

pub(crate) fn jit_map_string_type(ty: &Type) -> bool {
    matches!(ty, Type::Map { key, .. } if matches!(key.as_ref(), Type::String))
}

fn jit_map_composite_key_type(ty: &Type) -> bool {
    matches!(
        ty,
        Type::Map { key, .. }
            if matches!(key.as_ref(), Type::Tuple(_) | Type::Named(_))
    )
}

fn jit_map_composite_key_expr(expr: &TExpr) -> bool {
    let scalar_field = |field: &TExpr| {
        matches!(
            &field.ty,
            Type::Int | Type::IntN { .. } | Type::String | Type::Bool | Type::Char
        )
    };
    match (&expr.ty, &expr.kind) {
        (Type::Tuple(_), TExprKind::TupleLit { fields, .. }) => {
            fields.iter().all(|(_, field)| scalar_field(field))
        }
        (Type::Named(_), TExprKind::StructLit { fields, .. }) => {
            fields.iter().all(|(_, field, _)| scalar_field(field))
        }
        _ => false,
    }
}

fn jit_map_int_key_type(ty: &Type) -> bool {
    matches!(
        erase_runtime_qualifiers(ty),
        Type::Int | Type::InlineRange { .. }
    )
}

fn jit_map_string_int_type(ty: &Type) -> bool {
    matches!(
        ty,
        Type::Map { key, value, .. }
            if matches!(key.as_ref(), Type::String)
                && intish_ty(value)
    )
}

/// `Map<Int, V>` with scalar/handle values — Int keys share the i64 map heap ABI.
pub(crate) fn jit_map_int_type(ty: &Type) -> bool {
    matches!(
        ty,
        Type::Map { key, value, .. }
            if jit_map_int_key_type(key)
                && (jit_value_type(value) || matches!(value.as_ref(), Type::String))
    )
}

fn jit_map_resident_type(ty: &Type) -> bool {
    (jit_map_string_type(ty) || jit_map_int_type(ty) || jit_map_composite_key_type(ty))
        && !jit_map_intn_value_type(ty)
}

fn jit_map_intn_value_type(ty: &Type) -> bool {
    matches!(ty, Type::Map { value, .. } if matches!(value.as_ref(), Type::IntN { .. }))
}

pub(crate) fn jit_list_task_type(ty: &Type) -> bool {
    if let Type::List(inner) = ty {
        if let Type::Apply { name, args } = inner.as_ref() {
            return name == "Task" && args.len() == 1 && jit_concurrency_elem(&args[0]);
        }
    }
    false
}

pub(crate) fn jit_list_task_int_type(ty: &Type) -> bool {
    if let Type::List(inner) = ty {
        if let Type::Apply { name, args } = inner.as_ref() {
            return name == "Task" && args.len() == 1 && matches!(&args[0], Type::Int);
        }
    }
    false
}

pub(crate) fn jit_optional_scalar_type(ty: &Type) -> bool {
    // Option carrier ABI: IntN uses result-arena handles; other scalars and
    // named enums use the legacy packed carrier. Nested Option stays out to avoid
    // recursive jit_value_type → compound loops.
    matches!(
        ty,
        Type::Option(inner)
            if jit_scalar_type(inner)
                || matches!(
                    inner.as_ref(),
                    Type::Named(_)
                        | Type::String
                        | Type::Tuple(_)
                        | Type::List(_)
                        | Type::FixedList { .. }
                )
    )
}

pub(crate) fn user_type_name(ty: &Type) -> Option<&str> {
    match ty {
        Type::Named(n) if n != "Unit" => Some(n.as_str()),
        // D-PIN1=A: `Pin<T>` is a window onto a `T` place, not a record of its
        // own. Field layout, printing, and the record ABI all belong to `T`.
        Type::Apply { name, args }
            if name == jet_foundation::Syntax::TYPE_PIN && args.len() == 1 =>
        {
            user_type_name(&args[0])
        }
        Type::Apply { name, .. } => Some(name.as_str()),
        Type::Tagged { inner, .. } => user_type_name(inner),
        _ => None,
    }
}

pub(crate) fn record_type_key(ty: &Type) -> Option<String> {
    match ty {
        Type::Tuple(_) => Some(ty.name()),
        _ => user_type_name(ty).map(str::to_string),
    }
}

pub(crate) fn jit_struct_type(ty: &Type) -> bool {
    user_type_name(ty).is_some()
}

pub(crate) fn jit_list_record_type(ty: &Type) -> bool {
    matches!(
        ty,
        Type::List(elem) | Type::FixedList { elem, .. }
            if record_type_key(elem).is_some() || matches!(elem.as_ref(), Type::TraitObject(_))
    )
}

pub(crate) fn jit_tuple_type(ty: &Type) -> bool {
    // Named tuples lower to record handles (i64), same as structs — any field
    // shape that fits the record ABI (scalars, Option, String, nested records).
    matches!(
        ty,
        Type::Tuple(fields)
            if !fields.is_empty()
                && fields.iter().all(|(_, t)| {
                    matches!(
                        t.as_ref(),
                        Type::Int
                            | Type::IntN { .. }
                            | Type::InlineRange { .. }
                            | Type::Float
                            | Type::Float32
                            | Type::Bool
                            | Type::Char
                            | Type::String
                            | Type::Option(_)
                            | Type::Named(_)
                            | Type::Tuple(_)
                            | Type::List(_)
                        )
                        || jit_tuple_handle_field(t)
                })
    )
}

fn jit_tuple_handle_field(ty: &Type) -> bool {
    match ty {
        Type::Apply { name, .. } => matches!(
            name.as_str(),
            "CellReadGuard" | "CellEditGuard" | "SharedGuard" | "ViewMut"
        ),
        Type::Tagged { inner, .. } => jit_tuple_handle_field(inner),
        _ => false,
    }
}

pub(crate) fn jit_enum_type(ty: &Type) -> bool {
    user_type_name(ty).is_some() || matches!(ty, Type::Union(_))
}

fn jit_compound_type(ty: &Type) -> bool {
    jit_list_native_type(ty)
        || jit_list_of_int_list_type(ty)
        || jit_list_task_type(ty)
        || jit_list_record_type(ty)
        || jit_map_string_type(ty)
        || jit_map_int_type(ty)
        || matches!(
            ty,
            Type::List(inner) | Type::FixedList { elem: inner, .. }
                if jit_map_string_type(inner)
        )
        || jit_struct_type(ty)
        || jit_tuple_type(ty)
        || jit_enum_type(ty)
        || jit_optional_scalar_type(ty)
        || matches!(
            ty,
            Type::Apply { name, args }
                if args.len() == 1
                    && (matches!(
                        name.as_str(),
                        "Set"
                            | jet_foundation::Syntax::TYPE_QUEUE
                            | "Pool"
                            | "Id"
                            | "Stream"
                            | "ExpiringValue"
                            | "ExpiringSecret"
                            | jet_foundation::Syntax::TYPE_RANK
                            | "PriorityQueue"
                            | "Cache"
                            | "Ptr"
                    ) || (name == jet_foundation::Syntax::TYPE_TALLY && jit_bag_raw_key_candidate(&args[0])))
        )
        || matches!(ty, Type::Apply { name, args }
            if matches!(name.as_str(), "View" | "ComputeViewMut")
                && args.len() == 1
                && jit_value_type(&args[0]))
        || matches!(ty, Type::Named(name) if matches!(name.as_str(), jet_foundation::Syntax::TYPE_BITS | jet_foundation::Syntax::TYPE_BYTES))
        || matches!(ty, Type::Shared(_))
}

fn jit_concurrency_elem(ty: &Type) -> bool {
    matches!(ty, Type::Named(n) if n == "Unit") || jit_scalar_type(ty)
}

pub(crate) fn jit_concurrency_type(ty: &Type) -> bool {
    let Type::Apply { name, args } = ty else {
        return false;
    };
    matches!(name.as_str(), "Task" | "Receiver" | "Sender")
        && args.len() == 1
        && jit_concurrency_elem(&args[0])
}

pub(crate) fn jit_value_type(ty: &Type) -> bool {
    if ty.is_compute_tensor_family() {
        return true;
    }
    if let Some((base, _)) = ty.quantity_parts() {
        return jit_value_type(base);
    }
    match ty {
        Type::Tagged { inner, .. } => jit_value_type(inner),
        Type::Named(n)
            if n.chars()
                .next()
                .is_some_and(|c| c.is_ascii_uppercase()) =>
        {
            // Opaque i64 handles: std/user structs and enums (Date, Cmd,
            // FileReader, WatchEvent, GameScene, …).
            true
        }
        Type::Apply { name, args }
            if matches!(name.as_str(), "CellReadGuard" | "CellEditGuard")
                && args.len() == 1 =>
        {
            true
        }
        Type::Apply { name, .. }
            if name == jet_foundation::Syntax::TYPE_CHECKED_TEXT => true,
        Type::Apply { name, args }
            if name == jet_foundation::Syntax::TYPE_SHARED_GUARD && args.len() == 1 =>
        {
            true
        }
        Type::Apply { name, args }
            if matches!(
                name.as_str(),
                "Signal"
                    | "Derived"
                    | "Computed"
                    | "Effect"
                    | "Loadable"
                    | "Event"
                    | "AsyncEvent"
                    | "Hook"
                    | "DecisionHook"
                    | "Watch"
            ) && args.iter().all(jit_value_type) =>
        {
            true
        }
        Type::Int
        | Type::IntN { .. }
        | Type::InlineRange { .. }
        | Type::Float
        | Type::Float32
        | Type::Bool
        | Type::String
        | Type::Char => true,
        Type::Tuple(fields) if fields.is_empty() => true,
        Type::Fn { params, ret, .. } => {
            params.iter().all(jit_value_type)
                && ret.as_ref().is_none_or(|ret| jit_value_type(ret))
        }
        Type::Union(members) => members.iter().all(jit_value_type),
        Type::TraitObject(traits) => !traits.is_empty(),
        Type::Result { ok, err } => {
            jit_result_payload_type(ok.as_ref()) && jit_result_payload_type(err.as_ref())
        }
        other if jit_concurrency_type(other) => true,
        other if jit_compound_type(other) => true,
        _ => false,
    }
}

fn jit_cell_value_type(ty: &Type) -> bool {
    let ty = erase_runtime_qualifiers(ty);
    cell_value_map_keys_are_string(ty)
        && (matches!(ty, Type::Named(name) if name == "Unit")
        || matches!(ty, Type::Tuple(fields) if fields.is_empty())
        || super::types_meta::clif_ty(ty).is_some())
}

fn cell_value_map_keys_are_string(ty: &Type) -> bool {
    match erase_runtime_qualifiers(ty) {
        Type::Map { key, value, .. } => {
            matches!(erase_runtime_qualifiers(key), Type::String)
                && cell_value_map_keys_are_string(value)
        }
        Type::List(inner)
        | Type::FixedList { elem: inner, .. }
        | Type::Shared(inner)
        | Type::Option(inner)
        | Type::InlineRange { base: inner, .. }
        | Type::Quantity { base: inner, .. } => cell_value_map_keys_are_string(inner),
        Type::Result { ok, err } => {
            cell_value_map_keys_are_string(ok) && cell_value_map_keys_are_string(err)
        }
        Type::Apply { args, .. } => args.iter().all(cell_value_map_keys_are_string),
        Type::Tuple(fields) => fields
            .iter()
            .all(|(_, field)| cell_value_map_keys_are_string(field)),
        Type::Union(members) => members.iter().all(cell_value_map_keys_are_string),
        Type::Fn { .. } => true,
        _ => true,
    }
}

pub(crate) fn jit_result_payload_type(ty: &Type) -> bool {
    matches!(ty, Type::Named(n) if n == "Unit" || n == jet_foundation::Syntax::TYPE_ERR || n == jet_foundation::Syntax::TYPE_TASK_FAILURE)
        || jit_value_type(ty)
}

/// D-CAP9 / S58: which raw pointers this tier may mint, stated once for the
/// worklist gate and the recursive predicate that shadows it.
///
/// I1: an `#Unsafe` region is audited USER code, not a licence for the compiler
/// to give up, so `RawOf` and `Deref` are not refused for being raw. They are
/// refused for naming storage this tier does not own. `LowerCtx`'s
/// `TExprKind::RawOf` arm mints an address from a bare local's own Cranelift
/// stack slot — live for the whole frame, sized by the pointee's ABI — and
/// declines every other operand, because a non-place operand has no storage of
/// its own and a pointer to a copy is a wrong answer (that is what made
/// `*arena.alloc(7)` outlive `arena.reset()` instead of reporting R0802).
///
/// The `Ptr<_>` operand is the no-op cast a `*T.{*x}` literal wraps around its
/// `*x`; the lowering passes it through, and the walk gates the inner `*x` on
/// its own, so the answer here is unconditional.
///
/// D-MEM-SENTRY1 is why this admitted set is exactly the safe one: the Prelude
/// sentry kernel owns provenance, quarantine and R08xx, and this tier pushes no
/// `jet_sentry_scope`, so it can report nothing. Inside the admitted set there
/// is nothing to report — a frame-lived slot cannot be quarantined by an arena
/// reset and cannot be shorter than its own pointee — and every shape whose
/// answer the kernel does own (`from_addr(<literal>)`, `*<non-place>`) leaves
/// the set at the pointer instead of at the region.
fn resident_safe_raw_of(operand: &TExpr) -> bool {
    if matches!(&operand.ty, Type::Apply { name, args }
        if name == jet_foundation::Syntax::TYPE_PTR && args.len() == 1)
    {
        return true;
    }
    crate::lower_ctx::LowerCtx::raw_place_local(operand).is_some()
        && super::types_meta::clif_ty(&operand.ty).is_some()
}

/// The load half of [`resident_safe_raw_of`]: `p.*` needs a machine type to
/// land in (`LowerCtx`'s `TExprKind::Deref` arm asks `meta.clif_ty` for exactly
/// this). Provenance stays a lowering-time fact — it is a property of the SSA
/// value, not of the type — and the lowering refuses through it, which is a
/// deopt and never a wrong answer.
fn resident_safe_raw_deref(result: &Type) -> bool {
    super::types_meta::clif_ty(result).is_some()
}

enum ResidentSafeExprTask<'a> {
    Visit(&'a TExpr),
    FinishAll(usize),
}

/// Walk the recursive expression shapes on the heap before handing an
/// unsupported shape to the exact coverage predicate below.
///
/// Tier planning runs on the caller's ordinary thread.  A deep expression can
/// therefore exhaust that thread before Cranelift has a chance to classify the
/// function: `plan_tiers -> resident_safe_func_detail -> resident_safe_stmt
/// -> resident_safe_expr`.  The worklist changes only where the predicate's
/// child calls are made; every gate and child remains the same predicate as
/// `resident_safe_expr_recursive`.
pub(crate) fn resident_safe_expr(expr: &TExpr, callees: &HashSet<String>) -> bool {
    let mut tasks = vec![ResidentSafeExprTask::Visit(expr)];
    let mut results = Vec::new();
    while let Some(task) = tasks.pop() {
        match task {
            ResidentSafeExprTask::Visit(expr) => {
                let Some((gate, children)) = resident_safe_expr_work_item(expr, callees) else {
                    results.push(resident_safe_expr_recursive(expr, callees));
                    continue;
                };
                if !gate {
                    results.push(false);
                    continue;
                }
                let count = children.len();
                tasks.push(ResidentSafeExprTask::FinishAll(count));
                for child in children.into_iter().rev() {
                    tasks.push(ResidentSafeExprTask::Visit(child));
                }
            }
            ResidentSafeExprTask::FinishAll(count) => {
                let Some(start) = results.len().checked_sub(count) else {
                    results.push(false);
                    continue;
                };
                let value = results[start..].iter().all(|value| *value);
                results.truncate(start);
                results.push(value);
            }
        }
    }
    results.pop().unwrap_or(false)
}

/// The one `??` table (I8): which expressions a fallback makes the JIT lower.
///
/// `TExprKind::OrFallback`'s lowering builds the same fail block for either
/// carrier — the packed Option word and the result-arena handle — and its match
/// on `TOrFallback` is total in both: `Value` lowers the expression and jumps to
/// the merge, `Break`/`Continue` (and their labelled forms) go through
/// `emit_loop_fallback`, `Return` through `emit_lexical_exit`, and `Panic`
/// through `emit_rich_panic` (lower_ctx.rs `TExprKind::OrFallback`). So the
/// carrier does not change the answer and no fallback shape is refused; the only
/// question left is whether the expressions the fallback carries are resident.
///
/// The option side used to refuse `?? return …` while the result side admitted
/// it, and it also admitted `?? <value>` without ever asking about the value.
fn or_fallback_children<'a>(value: &'a TExpr, fallback: &'a TOrFallback) -> Vec<&'a TExpr> {
    let mut children = vec![value];
    match fallback {
        TOrFallback::Value(e) | TOrFallback::Return(Some(e)) => children.push(e),
        TOrFallback::Return(None)
        | TOrFallback::Panic { .. }
        | TOrFallback::Break
        | TOrFallback::Continue
        | TOrFallback::BreakLabel(_)
        | TOrFallback::ContinueLabel(_) => {}
    }
    children
}

fn resident_safe_expr_work_item<'a>(
    expr: &'a TExpr,
    callees: &HashSet<String>,
) -> Option<(bool, Vec<&'a TExpr>)> {
    match &expr.kind {
        TExprKind::Print(inner) => Some((true, vec![inner])),
        TExprKind::CoreCall {
            module,
            method,
            args,
            ..
        } => resident_safe_crypto_work_item(module, method, args),
        TExprKind::OrFallback { value, fallback } => {
            Some((true, or_fallback_children(value, fallback)))
        }
        TExprKind::ListLit(elems) => {
            let scalar_list = jit_list_native_type(&expr.ty)
                && elems.iter().all(|e| {
                    matches!(
                        &e.ty,
                        Type::Int
                            | Type::IntN { .. }
                            | Type::InlineRange { .. }
                            | Type::Float
                            | Type::String
                            | Type::Char
                    ) || jit_optional_scalar_type(&e.ty)
                });
            let nested_int_list = jit_list_of_int_list_type(&expr.ty);
            let nested_string_list = matches!(
                &expr.ty,
                Type::List(inner)
                    if jit_list_native_type(inner)
                        && matches!(inner.as_ref(), Type::List(elem) if matches!(elem.as_ref(), Type::String))
            );
            let task_list = jit_list_task_int_type(&expr.ty);
            let record_list = jit_list_record_type(&expr.ty);
            let named_or_union = matches!(
                &expr.ty,
                Type::List(elem) | Type::FixedList { elem, .. }
                    if matches!(elem.as_ref(), Type::Named(_) | Type::Union(_))
            );
            Some((
                scalar_list || nested_int_list || nested_string_list || task_list || record_list || named_or_union,
                elems.iter().collect(),
            ))
        }
        TExprKind::Try { inner, convert, .. } => Some((
            matches!(
                convert,
                TIR::TTryConvert::None
                    | TIR::TTryConvert::DefaultErr
                    | TIR::TTryConvert::Typed(_)
                    | TIR::TTryConvert::WidenUnion { .. }
            ),
            vec![inner],
        )),
        TExprKind::DistinctConvert { arg, .. }
        | TExprKind::DistinctRaw(arg)
        | TExprKind::Drop(arg)
        | TExprKind::MaterializeView(arg)
        | TExprKind::Present(arg)
        | TExprKind::Ok(arg)
        | TExprKind::Err(arg)
        | TExprKind::ResourceNew(arg)
        | TExprKind::Clone(arg) => Some((true, vec![arg])),
        // A measured unit scale (`UnitScaleProvenance::Measured`) makes TIR
        // carry `relative_uncertainty`, and AOT then wraps the converted value
        // through `jet_measurement_kernel_from_relative` so the result is a
        // Measurement pair, not a number (`emit/expressions.rs` UnitConvert).
        // `LowerCtx`'s UnitConvert arm destructures `arg, scale, offset,
        // rounding, fallible, ..` and never reads that field, so admitting it
        // here hands back a bare f64 where the program's type is a
        // Measurement — a wrong answer, not a stop. This arm SHADOWS
        // `resident_safe_expr_recursive`, which already carried the gate; the
        // two must state one law (the D-CHAINCMP1 lesson).
        TExprKind::UnitConvert {
            arg,
            relative_uncertainty,
            ..
        } => Some((relative_uncertainty.is_none(), vec![arg])),
        TExprKind::RawOf(arg) => Some((resident_safe_raw_of(arg), vec![arg])),
        TExprKind::Deref(arg) => Some((resident_safe_raw_deref(&expr.ty), vec![arg])),
        TExprKind::Borrow { place, .. } => Some((true, vec![place])),
        TExprKind::Unary { op, operand } => Some((
            jit_value_type(&expr.ty) && matches!(op, UnOp::Neg | UnOp::Not),
            vec![operand],
        )),
        TExprKind::Binary {
            op,
            overflow,
            lhs,
            rhs,
            ..
        } => {
            // `ProcessResult.signal` is a PACKED `Option<Int>` in the JIT
            // record (0 = absent, else payload + 1 — `Process.rs`
            // `alloc_process_result`), so a plain machine compare reads the
            // packed word and answers wrongly. The resident lowering covers
            // exactly the both-sides Eq/Ne shape, so gate on the field shape
            // rather than on the reported type — the same law
            // `resident_safe_expr_recursive` states, which this arm shadows.
            let packed_signal = is_packed_process_signal(lhs) || is_packed_process_signal(rhs);
            let gate = (if matches!(op, BinOp::And | BinOp::Or) {
                matches!(&lhs.ty, Type::Bool) && matches!(&rhs.ty, Type::Bool)
            } else if packed_signal {
                matches!(op, BinOp::Eq | BinOp::Ne)
                    && is_packed_process_signal(lhs)
                    && is_packed_process_signal(rhs)
            } else if matches!(&lhs.ty, Type::Option(_)) || matches!(&rhs.ty, Type::Option(_)) {
                resident_safe_option_binary(op, &lhs.ty, &rhs.ty)
            } else if *overflow {
                (intish_ty(&lhs.ty) || reactive_get_intish(lhs))
                    && (intish_ty(&rhs.ty) || reactive_get_intish(rhs))
            } else {
                true
            }) && jit_value_type(&expr.ty);
            Some((gate, vec![lhs, rhs]))
        }
        // D-CHAINCMP1: `ops` and `hooks` are per-COMPARISON facts — comparison
        // `i` joins `operands[i]` with `operands[i + 1]`, so `operands` holds
        // exactly one entry MORE than either. Walk adjacent operand pairs
        // zipped with their own op and hook, so a hook flag can never be read
        // against an operand index (and no index is taken at all). A hooked
        // pair dispatches through `Comparable.compare`; an unhooked pair lowers
        // to a native machine comparison, so BOTH of its sides must be a native
        // arithmetic type (`LowerCtx::lower_compare_chain`).
        TExprKind::CompareChain { operands, ops, hooks } => Some((
            jit_value_type(&expr.ty)
                && operands.len() == ops.len() + 1
                && hooks.len() == ops.len()
                && operands
                    .windows(2)
                    .zip(ops.iter().zip(hooks.iter()))
                    .all(|(pair, (op, hook))| {
                        matches!(op, BinOp::Lt | BinOp::Gt | BinOp::Le | BinOp::Ge)
                            && (*hook
                                || pair.iter().all(|operand| {
                                    matches!(
                                        &operand.ty,
                                        Type::Int
                                            | Type::IntN { .. }
                                            | Type::InlineRange { .. }
                                            | Type::Float
                                    )
                                }))
                    }),
            operands.iter().collect(),
        )),
        TExprKind::StrLit(parts) if jit_value_type(&expr.ty) => Some((
            true,
            parts
                .iter()
                .filter_map(|part| match part {
                    TStrPart::Lit(_) => None,
                    TStrPart::Interp(expr, _) => Some(expr),
                })
                .collect(),
        )),
        TExprKind::Call { name, args, .. } => {
            let gate = callees.contains(name)
                && args.iter().all(resident_safe_call_arg_gate);
            Some((gate, args.iter().map(|arg| &arg.value).collect()))
        }
        TExprKind::ModuleCall { form, args, .. } => {
            let target = match form {
                TModuleCallForm::Qualified { rust_mod, rust_fn } => {
                    format!("{rust_mod}::{rust_fn}")
                }
                TModuleCallForm::InlineMangled { mangled } => mangled.clone(),
            };
            let gate = callees.contains(&target)
                && args.iter().all(resident_safe_call_arg_gate);
            Some((gate, args.iter().map(|arg| &arg.value).collect()))
        }
        TExprKind::MethodCall { recv, args, .. }
        | TExprKind::FnFieldCall { recv, args, .. } => Some((
            args.iter().all(resident_safe_call_arg_gate),
            std::iter::once(recv.as_ref())
                .chain(args.iter().map(|arg| &arg.value))
                .collect(),
        )),
        TExprKind::StaticCall { args, .. } => Some((
            (!matches!(
                &expr.ty,
                Type::Apply { name, args }
                    if name == "Cell"
                        && args.first().is_some_and(|ty| !jit_cell_value_type(ty))
            )) && args.iter().all(resident_safe_call_arg_gate),
            args.iter().map(|arg| &arg.value).collect(),
        )),
        _ => None,
    }
}

/// I8/I9: admission is not a second policy.
///
/// `LowerCtx::lower_crypto_core_call_values` accepts a call exactly when
/// `LowerCtx::crypto_core_arity` names the pair and the arity matches, then
/// marshals it into the host that reaches the same Prelude symbol AOT embeds —
/// so consulting that one table here is the whole gate (the
/// `service_core_arity` treatment). This used to be a hand-written second copy
/// of the pair list, and it lagged the lowering by eight rows: `hkdf_sha256`,
/// `constant_time_equal`, `constant_time_equal_bytes`, `__secret_from_bytes`,
/// `__password_text`, `x25519_public`, `x25519_shared` and `file_seal` all had
/// host rows, and every program that used one was refused as an unsupported
/// construct (`examples/features/crypto/random_api_split.jet`).
///
/// `core.crypto.random.bytes` is the one crypto call that does NOT lower
/// through that function, so it keeps its own row here.
fn resident_safe_crypto_work_item<'a>(
    module: &str,
    method: &str,
    args: &'a [TExpr],
) -> Option<(bool, Vec<&'a TExpr>)> {
    let children = || -> Vec<&'a TExpr> { args.iter().collect() };
    if module == "core.crypto.random" {
        return Some((method == "bytes" && args.len() == 1, children()));
    }
    if !matches!(module, "core.crypto" | "core.crypto.expert") {
        return None;
    }
    Some((
        crate::lower_ctx::LowerCtx::crypto_core_arity(module, method) == Some(args.len()),
        children(),
    ))
}

/// S48: the concrete argument shapes that may enter a trait value slot.
///
/// `LowerCtx::lower_call_arg` boxes such an argument into the two-slot
/// `{type_id, concrete}` record `lower_trait_object_method` dispatches on, and
/// `LowerCtx::lower_trait_object_box` is the one writer of that shape (shared
/// with the literal-in-a-trait-slot arm). Both slots are I64 cells, so the
/// concrete has to BE an I64 record handle.
///
/// Deliberately meta-free and NARROWER than `record_type_key`: that key also
/// answers for `Type::Tuple`, which has no `struct_type_id` position at all,
/// and for a `Type::Apply` instantiation whose id is keyed by a mangled name
/// this file cannot form. A plain nominal name is the shape whose id
/// `struct_type_id` resolves. Anything this gate cannot resolve without meta is
/// a DEOPT — the lowering refuses through `struct_type_id` — never an ICE, and
/// never a bare concrete record handed to a trait parameter.
///
/// Refusing outright (card #2053) deopted every program with an S48 argument,
/// `print_area(one)` in `examples/features/types/traits.jet` included.
fn resident_safe_trait_arg(arg: &TCallArg) -> bool {
    arg.box_as_trait.is_none()
        || matches!(&arg.value.ty, Type::Named(name) if name != "Unit")
}

fn resident_safe_call_arg_gate(arg: &TCallArg) -> bool {
    if arg.arc_clone {
        return false;
    }
    if !resident_safe_trait_arg(arg) {
        return false;
    }
    let ty = &arg.value.ty;
    let handle_pass = jit_value_type(ty)
        || jit_struct_type(ty)
        || jit_tuple_type(ty)
        || matches!(
            ty,
            Type::String
                | Type::List(_)
                | Type::FixedList { .. }
                | Type::Option(_)
                | Type::Map { .. }
        );
    if (arg.borrow || arg.mut_borrow) && !handle_pass {
        return false;
    }
    if arg.clone {
        let clone_ok = matches!(
            ty,
                Type::Int
                | Type::Float
                | Type::Bool
                | Type::Char
                | Type::String
                | Type::Option(_)
                | Type::IntN { .. }
                | Type::InlineRange { .. }
                | Type::Float32
        ) || jit_struct_type(ty)
            || jit_compound_type(ty)
            || jit_tuple_type(ty)
            || jit_list_native_type(ty)
            || jit_list_record_type(ty)
            || jit_map_string_type(ty);
        if !clone_ok {
            return false;
        }
    }
    !arg.widen_to_vec
        || jit_list_native_type(ty)
        || matches!(ty, Type::FixedList { .. })
}

fn resident_safe_expr_recursive(expr: &TExpr, callees: &HashSet<String>) -> bool {
    match &expr.kind {
        TExprKind::Print(inner) => {
            resident_safe_expr(inner, callees)
        }
        TExprKind::Call { name, args, .. } => {
            if !callees.contains(name) {
                return false;
            }
            args.iter().all(|a| resident_safe_call_arg(a, callees))
        }
        TExprKind::ModuleCall { form, args, .. } => {
            let target = match form {
                TModuleCallForm::Qualified { rust_mod, rust_fn } => {
                    format!("{rust_mod}::{rust_fn}")
                }
                TModuleCallForm::InlineMangled { mangled } => mangled.clone(),
            };
            callees.contains(&target)
                && args
                    .iter()
                    .all(|arg| resident_safe_call_arg(arg, callees))
        }
        TExprKind::CoreCall {
            module,
            method,
            args,
            ..
        } => {
            // `core.crypto`, `core.crypto.expert` and `core.crypto.random` are
            // answered by `resident_safe_crypto_work_item`, which
            // `resident_safe_expr` consults BEFORE reaching this predicate. The
            // three arms that used to be repeated here were therefore
            // unreachable, and they had already drifted from both the work item
            // and the lowering — the exact way this file has produced wrong
            // refusals before. One law, one arm.
            if module == "core.auth" && matches!(method.as_str(), "verify_jwt" | "verify_paseto") {
                return (3..=7).contains(&args.len())
                    && args.iter().all(|arg| resident_safe_expr(arg, callees));
            }
            if module == "core.auth" {
                return match method.as_str() {
                    "register_user" if args.len() == 2 => {
                        args.iter().all(|arg| resident_safe_expr(arg, callees))
                    }
                    "oauth_begin" if args.len() == 1 => {
                        args.iter().all(|arg| resident_safe_expr(arg, callees))
                    }
                    "password_login" | "oauth_finish" if args.len() == 4 => {
                        args.iter().all(|arg| resident_safe_expr(arg, callees))
                    }
                    "session_validate" if args.len() == 2 => {
                        args.iter().all(|arg| resident_safe_expr(arg, callees))
                    }
                    "magic_link_issue" | "magic_link_consume" if args.len() == 3 => {
                        args.iter().all(|arg| resident_safe_expr(arg, callees))
                    }
                    "session_show" | "session_user" | "session_cookie" | "session_id"
                        if args.len() == 1 =>
                    {
                        resident_safe_expr(&args[0], callees)
                    }
                    _ => false,
                };
            }
            if module == "core.crypto.vault" {
                return !args.is_empty()
                    && args.iter().all(|arg| resident_safe_expr(arg, callees));
            }
            if module == "core.text" {
                return match method.as_str() {
                    "lower" | "upper" | "trim" | "scalar_count" | "byte_count" | "graphemes"
                    | "words" | "sentences" | "nfc" | "nfkc" | "nfd" | "nfkd"
                    | "display_width" | "is_alphabetic" | "is_numeric" | "inspect" | "char_indices"
                        if args.len() == 1 =>
                    {
                        matches!(&args[0].ty, Type::String) && resident_safe_expr(&args[0], callees)
                    }
                    "display_width" if args.len() == 2 => {
                        matches!(&args[0].ty, Type::String)
                            && matches!(&args[1].ty, Type::Named(n) if n == "TextWidth")
                            && resident_safe_expr(&args[0], callees)
                            && resident_safe_expr(&args[1], callees)
                    }
                    "caseless_eq" | "starts_any" if args.len() == 2 => {
                        args.iter().all(|a| resident_safe_expr(a, callees))
                    }
                    "pad_start" | "center" if args.len() == 3 => {
                        args.iter().all(|a| resident_safe_expr(a, callees))
                    }
                    _ => false,
                };
            }
            if module.starts_with("core.data.sketch.") {
                return match (module.as_str(), method.as_str(), args.len()) {
                    ("core.data.sketch.hll" | "core.data.sketch.tdigest" | "core.data.sketch.cms", "new", 0) => {
                        true
                    }
                    ("core.data.sketch.reservoir", "new", 1) => {
                        resident_safe_expr(&args[0], callees)
                    }
                    _ => false,
                };
            }
            if module == "core.args" && method == "spec" {
                return args.is_empty();
            }
            if module == "core.text" {
                return matches!(args.as_slice(), [arg]
                    if matches!(
                        method.as_str(),
                        "scalar_count"
                            | "byte_count"
                            | "is_ascii"
                            | "lower"
                            | "upper"
                            | "scalars"
                    ) && matches!(&arg.ty, Type::String)
                        && resident_safe_expr(arg, callees));
            }
            if module == "core.term" {
                return match method.as_str() {
                    // `read_key` is nullary and lands on the shared Prelude
                    // kernel through one host shim, so the resident tier runs
                    // the same key decode AOT does (I9).
                    "args" | "readline" | "buffered" | "read_key" => args.is_empty(),
                    "print" | "eprint" | "take"
                    | "read_until" | "binread" | "input" | "confirm" | "input_secret"
                    | "read_all_input" | "stdin" | "stdout" | "stderr"
                    | "terminal_width" | "terminal_height" => {
                        args.iter().all(|arg| resident_safe_expr(arg, callees))
                    }
                    "binwrite" | "choose" | "style" | "style_force" => {
                        args.len() == 2
                            && args.iter().all(|arg| resident_safe_expr(arg, callees))
                    }
                    "progress" if (1..=3).contains(&args.len()) => {
                        args.iter().all(|arg| resident_safe_expr(arg, callees))
                    }
                    _ => false,
                };
            }
            if module == "core.reflect" && method == "of" {
                return args.len() == 1 && resident_safe_expr(&args[0], callees);
            }
            if module == "core.testing" {
                return match method.as_str() {
                    "temp_dir" | "fake_clock" | "fake_rng" | "fake_data" if args.len() == 1 => {
                        resident_safe_expr(&args[0], callees)
                    }
                    "snap" | "golden" if args.len() == 2 => {
                        args.iter().all(|a| resident_safe_expr(a, callees))
                    }
                    "fixture" if args.len() == 1 => resident_safe_expr(&args[0], callees),
                    _ => false,
                };
            }
            if module == "core.data" {
                return match method.as_str() {
                    "status" if args.is_empty() => true,
                    "csv" | "json" | "count" | "mean" | "sum" | "min" | "max" | "median"
                    | "variance" | "stddev" | "describe" | "bar_text" | "bar_svg"
                    | "line_text" | "line_svg"
                    | "require_bridge" | "table" | "rows" | "schema" | "series"
                    | "missing_count" | "lazy" | "collect" | "plan" | "values"
                        if args.len() == 1 =>
                    {
                        resident_safe_expr(&args[0], callees)
                    }
                    "line_text" | "line_svg" if args.len() == 2 => {
                        args.iter().all(|arg| resident_safe_expr(arg, callees))
                    }
                    "quantile" | "filter" | "sort_by" | "rolling_mean" if args.len() == 2 => {
                        // filter/sort_by carry lambdas — still resident-safe when
                        // the list + callable lower.
                        args.iter().all(|a| resident_safe_expr(a, callees))
                    }
                    "group_count" if args.len() == 2 => {
                        args.iter().all(|a| resident_safe_expr(a, callees))
                    }
                    "group_sum" | "group_mean" if args.len() == 3 => {
                        args.iter().all(|a| resident_safe_expr(a, callees))
                    }
                    "inner_join" | "left_join" if args.len() == 4 => {
                        args.iter().all(|a| resident_safe_expr(a, callees))
                    }
                    "pivot_sum" if args.len() == 4 => {
                        args.iter().all(|a| resident_safe_expr(a, callees))
                    }
                    "lazy_filter" | "lazy_sort_by" if args.len() == 2 => {
                        args.iter().all(|a| resident_safe_expr(a, callees))
                    }
                    "csv_reader" if args.len() == 2 => {
                        args.iter().all(|a| resident_safe_expr(a, callees))
                    }
                    _ => false,
                };
            }
            if module == "core.db" {
                let supported = match method.as_str() {
                    "open_memory" => args.is_empty(),
                    "open" | "params" => args.len() == 1,
                    "policy" | "row_int" | "row_text" => args.len() == 2,
                    "migrate" | "transaction" => args.len() == 3,
                    _ => false,
                };
                return supported && args.iter().all(|arg| resident_safe_expr(arg, callees));
            }
            if module == "core.compute" {
                return resident_safe_compute_call(method, args, &expr.ty, callees);
            }
            // I8/I9: admission is not a second policy. `lower_service_core_call`
            // (lower_ctx.rs) accepts a call exactly when `service_core_arity`
            // names the pair and the arity matches, then marshals it into the
            // `service_call` / `service_call_bool` host that reaches the same
            // Prelude symbol AOT embeds. Consulting that one table here is the
            // whole predicate; this arm previously restated all ~60 rows by
            // hand, so a new lowered method silently stayed deopted and a
            // removed one silently over-admitted.
            if matches!(module.as_str(), "core.service" | "core.services" | "core.sync")
                || ((module == "app" || module == "core.web") && method == "sync")
            {
                return crate::lower_ctx::LowerCtx::service_core_arity(module, method)
                    == Some(args.len())
                    && args.iter().all(|arg| resident_safe_expr(arg, callees));
            }
            if module == "core.mem" {
                // D-MEM-SENTRY1: the shared Prelude sentry kernel owns gate
                // state, provenance, quarantine, poison and R08xx reporting,
                // and this tier pushes no `jet_sentry_scope`, so it can report
                // nothing. That is a reason to keep the shapes the kernel
                // ANSWERS off this tier — not a reason to refuse raw memory as
                // a category (I1: an audited region is audited user code).
                //
                // `RawOf`, `Deref`, and `address_of` on a bare local have
                // resident lowerings for values whose storage is owned by the
                // current frame. Volatile access is different: its answer is
                // owned by the shared sentry Prelude, including gate state and
                // R08xx reporting. Keep those calls on canonical TIR so the
                // resident engine cannot grow a second memory policy.
                //
                // Every raw shape whose answer the kernel DOES own leaves the
                // admitted set at the pointer instead: `from_addr(<literal>)`
                // records no provenance and `*<non-place>` is refused by
                // `resident_safe_raw_of`, which is what keeps R0801 on
                // `examples/features/memory/unsafe_sentries_provenance.jet` and
                // R0802 on `examples/features/memory/unsafe_sentries.jet`.
                return match (method.as_str(), args.as_slice()) {
                    // Bare local only. That branch mints the local's own
                    // Cranelift stack slot, which is a real machine address like
                    // AOT's. The other branch mints the synthetic identity
                    // `TIR::stable_place_address` gives a place with no stable
                    // address — the interpreter agrees with it, AOT does not, so
                    // admitting it would put an I9 divergence on this tier
                    // instead of leaving it where it already is
                    // (`examples/features/memory/pin.jet`, a field projection).
                    ("address_of", [place]) => {
                        crate::lower_ctx::LowerCtx::raw_place_local(place).is_some()
                            && super::types_meta::clif_ty(&place.ty).is_some()
                            && resident_safe_expr(place, callees)
                    }
                    ("volatile_read", [_]) | ("volatile_write", [_, _]) => false,
                    _ => false,
                };
            }
            if (module == "app" || module == "core.web")
                && matches!(
                    method.as_str(),
                    "auth" | "auth_oauth" | "auth_routes" | "auth_show"
                )
            {
                return match (method.as_str(), args.len()) {
                    ("auth", 1) | ("auth_routes" | "auth_show", 1) => {
                        args.iter().all(|arg| resident_safe_expr(arg, callees))
                    }
                    ("auth_oauth", 2) => args.iter().all(|arg| resident_safe_expr(arg, callees)),
                    _ => false,
                };
            }
            if module == "app" {
                let supported = match method.as_str() {
                    "live" | "signal_push" => args.len() == 2,
                    "subscribe" | "invalidate" | "transact_invalidate" | "live_get"
                    | "live_show" => args.len() == 1,
                    "live_stats" => args.is_empty(),
                    _ => false,
                };
                return supported && args.iter().all(|arg| resident_safe_expr(arg, callees));
            }
            if module == "core.web"
                && matches!(
                    method.as_str(),
                    "live"
                        | "subscribe"
                        | "invalidate"
                        | "transact_invalidate"
                        | "signal_push"
                        | "live_get"
                        | "live_show"
                        | "live_stats"
                )
            {
                let supported = match method.as_str() {
                    "live" | "signal_push" => args.len() == 2,
                    "subscribe" | "invalidate" | "transact_invalidate" | "live_get"
                    | "live_show" => args.len() == 1,
                    "live_stats" => args.is_empty(),
                    _ => false,
                };
                return supported && args.iter().all(|arg| resident_safe_expr(arg, callees));
            }
            if module == "core.tasks" && method == "channel" {
                return args.len() <= 1 && args.iter().all(|a| resident_safe_expr(a, callees));
            }
            if module == "core.tasks" && matches!(method.as_str(), "after" | "interval") {
                let Some(duration) = args.first() else {
                    return false;
                };
                let value_safe = args.get(1).map_or(true, |value| {
                    matches!(&value.ty, Type::Int) && resident_safe_expr(value, callees)
                });
                return args.len() <= 2
                    && matches!(&duration.ty, Type::Named(name) if name == "Duration")
                    && resident_safe_expr(duration, callees)
                    && value_safe;
            }
            if module == "core.time" && matches!(method.as_str(), "now" | "sleep") {
                return match (method.as_str(), args.len()) {
                    ("now", 0) => true,
                    ("sleep", 1) => {
                        matches!(&args[0].ty, Type::Named(name) if name == "Duration")
                            && resident_safe_expr(&args[0], callees)
                    }
                    _ => false,
                };
            }
            if module == "core.task" && method == "timeout" {
                return args.len() == 1
                    && matches!(&args[0].ty, Type::Named(name) if name == "Duration")
                    && resident_safe_expr(&args[0], callees);
            }
            if module == "core.game" && method == "run" {
                return !args.is_empty()
                    && args.len() <= 3
                    && args.iter().all(|a| resident_safe_expr(a, callees));
            }
            if module == "core.game.raylib" {
                return match method.as_str() {
                    "window_open" => args.len() == 3,
                    "color" => args.len() == 4,
                    "set_target_fps" | "key_down" | "begin_drawing" | "clear_background"
                    | "close_window" | "window_should_close" | "window_ready" | "load_sound"
                    | "play_sound" => args.len() == 1,
                    "draw_rectangle" | "draw_text" => args.len() == 5,
                    "end_drawing" => args.is_empty(),
                    _ => false,
                } && args.iter().all(|a| resident_safe_expr(a, callees));
            }
            if module == "core.math.random"
                && method == "weighted_pick"
                && args.first().is_some_and(|items| jit_list_intn_type(&items.ty))
            {
                return false;
            }
            // Other core modules retain their existing resident coverage. core.text
            // alone is exact above: unsupported methods cannot fail into fallback.
            resident_safe_expr_list(args, callees)
        }
        TExprKind::CoreClosureCall { kind } => match kind {
            TCoreClosureKind::Spawn { .. } => true,
            TCoreClosureKind::OnInterrupt { callback } => {
                resident_safe_expr(callback, callees)
            }
            TCoreClosureKind::UiButtonOnClick {
                label,
                executable,
                ..
            } => {
                resident_safe_expr(label, callees)
                    && match &executable.executable {
                        TIR::TLambdaBody::Expr(e) => {
                            if resident_safe_expr(e, callees) {
                                true
                            } else {
                                let _ = expr_kind_name(&e.kind);
                                false
                            }
                        }
                        TIR::TLambdaBody::Block(stmts) => {
                            stmts.iter().all(|s| resident_safe_stmt(s, callees))
                        }
                        TIR::TLambdaBody::SharedBlock(stmts) => {
                            stmts.iter().all(|s| resident_safe_stmt(s, callees))
                        }
                    }
            }
            TCoreClosureKind::ReactiveDerived { executable, .. }
            | TCoreClosureKind::ReactiveEffect { executable, .. }
            | TCoreClosureKind::UiReactiveRender { executable, .. } => {
                match &executable.executable {
                    TIR::TLambdaBody::Expr(e) => {
                        if resident_safe_expr(e, callees) {
                            true
                        } else {
                            // Keep false; tag surfaces via Let init detail when needed.
                            let _ = expr_kind_name(&e.kind);
                            false
                        }
                    }
                    TIR::TLambdaBody::Block(stmts) => {
                        stmts.iter().all(|s| resident_safe_stmt(s, callees))
                    }
                    TIR::TLambdaBody::SharedBlock(stmts) => {
                        stmts.iter().all(|s| resident_safe_stmt(s, callees))
                    }
                }
            }
            TCoreClosureKind::Guard { executable, .. }
            | TCoreClosureKind::OnCommit { executable, .. }
            | TCoreClosureKind::OnRollback { executable, .. } => {
                match &executable.executable {
                    TIR::TLambdaBody::Expr(e) => resident_safe_expr(e, callees),
                    TIR::TLambdaBody::Block(stmts) => {
                        stmts.iter().all(|s| resident_safe_stmt(s, callees))
                    }
                    TIR::TLambdaBody::SharedBlock(stmts) => {
                        stmts.iter().all(|s| resident_safe_stmt(s, callees))
                    }
                }
            }
            _ => false,
        },
        TExprKind::HandleMethod { recv, op, args } => {
            let args_ok = match op {
                // Frame / click callbacks are registered via JIT spawn-sites;
                // the TIR lambda arg is not the resident-lowered body.
                THandleOp::GameSceneOnFrame => args.len() <= 1,
                THandleOp::UiBackendMethod { method } if method == "on_click" => {
                    args.len() == 2
                        && resident_safe_expr(&args[0], callees)
                        && matches!(&args[1].kind, TExprKind::Lambda(_))
                }
                _ => args.iter().all(|a| resident_safe_expr(a, callees)),
            };
            resident_safe_expr(recv, callees)
                && args_ok
                && resident_safe_handle_op(op, recv, args)
        }
        // Shadowed by `resident_safe_expr_work_item`; both read the one table.
        TExprKind::OrFallback { value, fallback } => or_fallback_children(value, fallback)
            .into_iter()
            .all(|child| resident_safe_expr(child, callees)),
        TExprKind::MapLit(entries) => {
            jit_map_resident_type(&expr.ty)
                && entries.iter().all(|(k, v)| {
                    let key_ok = if jit_map_composite_key_type(&expr.ty) {
                        jit_map_composite_key_expr(k)
                    } else if jit_map_int_type(&expr.ty) {
                        jit_map_int_key_type(&k.ty)
                    } else {
                        matches!(&k.ty, Type::String)
                    };
                    key_ok
                        && jit_value_type(&v.ty)
                        && resident_safe_expr(k, callees)
                        && resident_safe_expr(v, callees)
                })
        }
        TExprKind::ListLit(elems) => {
            (jit_list_native_type(&expr.ty)
                && elems.iter().all(|e| {
                    (matches!(
                        &e.ty,
                        Type::Int
                            | Type::IntN { .. }
                            | Type::InlineRange { .. }
                            | Type::Float
                            | Type::String
                            | Type::Char
                    ) || jit_optional_scalar_type(&e.ty))
                        && resident_safe_expr(e, callees)
                }))
                || (jit_list_of_int_list_type(&expr.ty)
                    && elems.iter().all(|e| resident_safe_expr(e, callees)))
                || (matches!(
                    &expr.ty,
                    Type::List(inner) if jit_list_native_type(inner)
                        && matches!(inner.as_ref(), Type::List(elem) if matches!(elem.as_ref(), Type::String))
                ) && elems.iter().all(|e| resident_safe_expr(e, callees)))
                || (jit_list_task_type(&expr.ty)
                    && elems.iter().all(|e| resident_safe_expr(e, callees)))
                || (jit_list_record_type(&expr.ty)
                    && elems.iter().all(|e| resident_safe_expr(e, callees)))
                || (matches!(
                    &expr.ty,
                    Type::List(elem) | Type::FixedList { elem, .. }
                        if matches!(elem.as_ref(), Type::Named(_) | Type::Union(_))
                ) && elems.iter().all(|e| resident_safe_expr(e, callees)))
        }
        TExprKind::ListSpread { parts } => {
            matches!(&expr.ty, Type::List(_) | Type::FixedList { .. })
                && parts.iter().all(|part| match part {
                    ListSpreadPart::Elem(elem) => resident_safe_expr(elem, callees),
                    ListSpreadPart::Spread(list) => {
                        (jit_list_native_type(&list.ty) || jit_list_record_type(&list.ty))
                            && resident_safe_expr(list, callees)
                    }
                })
        }
        // D-SOA-TIER1=A: a `#Layout(columnar)` list is held on this tier as its
        // logical rows, so the literal carries the same record-list gate a plain
        // `[S]` literal does. The two reads the layout defines are resident
        // because their ONE host marshals those rows into THE shared Prelude
        // column store and reads them with the Prelude's own gather
        // (`lower_ctx::lower_columnar_gather`), and the row it returns is an
        // ordinary arena record — so the whole-record read is a record handle and
        // the fused field read is the existing record-slot accessor. Each gate
        // below admits exactly what that lowering compiles: widening one without
        // its arm would be a Cranelift rejection of generated code (I2).
        TExprKind::ColumnarListLit { elems, .. } => {
            jit_list_record_type(&expr.ty) && elems.iter().all(|e| resident_safe_expr(e, callees))
        }
        TExprKind::ColumnarGather { base, index, .. } => {
            jit_list_record_type(&base.ty)
                && matches!(&index.ty, Type::Int)
                && record_type_key(&expr.ty).is_some()
                && resident_safe_expr(base, callees)
                && resident_safe_expr(index, callees)
        }
        TExprKind::ColumnarColumnRead { base, index, .. } => {
            // The cell lands in the field's own carrier through `record_slot`,
            // which reads a `String` field by handle and every other field by its
            // CLIF type. Gate on exactly that, so no cell shape reaches a lowering
            // that has no accessor for it.
            jit_list_record_type(&base.ty)
                && matches!(&index.ty, Type::Int)
                && (matches!(&expr.ty, Type::String)
                    || super::types_meta::clif_ty(&expr.ty).is_some())
                && resident_safe_expr(base, callees)
                && resident_safe_expr(index, callees)
        }
        TExprKind::Index {
            base,
            index,
            is_map,
            ..
        } => {
            if *is_map {
                let key_ok = if jit_map_composite_key_type(&base.ty) {
                    jit_map_composite_key_expr(index)
                } else if jit_map_int_type(&base.ty) {
                    jit_map_int_key_type(&index.ty)
                } else {
                    jit_map_string_type(&base.ty) && matches!(&index.ty, Type::String)
                };
                key_ok
                    && !jit_map_intn_value_type(&base.ty)
                    && resident_safe_expr(base, callees)
                    && resident_safe_expr(index, callees)
            } else {
                (jit_list_native_type(&base.ty)
                    || jit_list_record_type(&base.ty)
                    || jit_list_iter_elem_type(&base.ty).is_some()
                    || jit_closure_elem_type(&base.ty).is_some()
                    || jit_float_view_type(&base.ty))
                    && matches!(&index.ty, Type::Int)
                    && resident_safe_expr(base, callees)
                    && resident_safe_expr(index, callees)
            }
        }
        TExprKind::Slice {
            base,
            start,
            end,
            range,
            ..
        } => {
            (jit_list_native_type(&base.ty)
                || jit_list_record_type(&base.ty)
                || base.ty.is_compute_tensor_family())
                && resident_safe_expr(base, callees)
                && range.as_deref().map_or_else(
                    || {
                        matches!(&start.ty, Type::Int)
                            && matches!(&end.ty, Type::Int)
                            && resident_safe_expr(start, callees)
                            && resident_safe_expr(end, callees)
                    },
                    |range| {
                        matches!(&range.ty, Type::Named(name) if name == jet_foundation::Syntax::TYPE_RANGE)
                            && resident_safe_expr(range, callees)
                    },
                )
        }
        TExprKind::BuiltinMethod { recv, op, args } => {
            resident_safe_builtin_op(op, recv, args, callees)
        }
        TExprKind::NumericMethod { recv, op } => {
            resident_safe_expr(recv, callees)
                && match op {
                    TNumericOp::Predicate(name) => matches!(&recv.ty, Type::Float)
                        && matches!(name.as_str(), "is_nan" | "is_infinite" | "is_finite"),
                    TNumericOp::BitCount { method: name, width } => {
                        (matches!(&recv.ty, Type::IntN { bits, .. } if u32::from(*bits) == *width)
                            || (*width == 64 && matches!(&recv.ty, Type::Int)))
                            && matches!(
                                name.as_str(),
                                "count_ones"
                                    | "count_zeros"
                                    | "leading_zeros"
                                    | "trailing_zeros"
                            )
                    }
                    TNumericOp::ToShow => {
                        matches!(&recv.ty, Type::Int | Type::IntN { .. } | Type::InlineRange { .. } | Type::Float)
                    },
                    TNumericOp::CastAs { dst_rust } => {
                        recv.ty.is_numeric()
                            && matches!(dst_rust.as_str(), "i8" | "i16" | "i32" | "i64" | "u8" | "u16" | "u32" | "u64" | "f32" | "f64")
                    }
                    TNumericOp::CheckedIntToFloat { .. } => recv.ty.is_integer(),
                    TNumericOp::FloatToInt { .. } | TNumericOp::FloatNarrow { .. } => recv.ty.is_float(),
                    TNumericOp::TryFrom { .. } => recv.ty.is_integer(),
                    TNumericOp::InlineRange { .. } => recv.ty.is_integer(),
                    TNumericOp::Origin { .. } => true,
                }
        }
        TExprKind::DistinctConvert { arg, .. } | TExprKind::DistinctRaw(arg) => {
            resident_safe_expr(arg, callees)
        }
        TExprKind::UnitConvert {
            arg,
            relative_uncertainty,
            ..
        } => relative_uncertainty.is_none() && resident_safe_expr(arg, callees),
        TExprKind::PreciseBuiltin {
            type_name,
            func,
            args,
        } => {
            ((type_name == "Decimal"
                    && matches!(
                        (func.as_str(), args.len()),
                        ("from_str" | "to_string", 1)
                            | ("add" | "sub" | "mul" | "equal", 2)
                            | ("to_float", 1)
                    ))
                || (type_name == "Fraction"
                    && matches!(
                        (func.as_str(), args.len()),
                        (
                            "to_string" | "numerator" | "denominator" | "to_float" | "is_zero",
                            1
                        ) | ("add" | "sub" | "mul" | "div" | "equal", 2)
                    ))
                || (type_name == jet_foundation::Syntax::TYPE_COMPLEX
                    && matches!(
                        (func.as_str(), args.len()),
                        ("from_parts", 2)
                            | ("add" | "sub" | "mul" | "div", 2)
                            | ("abs" | "to_string", 1)
                    )))
                && args.iter().all(|arg| resident_safe_expr(arg, callees))
        }
        TExprKind::StructLit { fields, .. } => {
            (jit_struct_type(&expr.ty)
                || matches!(&expr.ty, Type::TraitObject(_))
                || matches!(&expr.ty, Type::Named(name) if name == jet_foundation::Syntax::TYPE_RANGE))
                && fields
                    .iter()
                    .all(|(_, v, _)| resident_safe_expr(v, callees))
        }
        TExprKind::DataEntriesToMap(local) => {
            local.generated
                && !local.deref
                && local
                    .name
                    .starts_with(&jet_foundation::Names::mangle_generated("obj"))
        }
        TExprKind::TupleLit { fields, .. } => {
            jit_tuple_type(&expr.ty) && resident_safe_tuple_fields(fields, callees)
        }
        TExprKind::Field { recv, .. } => match &recv.kind {
            TExprKind::Local(_)
                if matches!(&recv.ty, Type::Tuple(_))
                    && matches!(
                        &expr.ty,
                        Type::Apply { name, args }
                            if matches!(name.as_str(), "ViewMut" | "ComputeViewMut")
                                && args.len() == 1
                    ) =>
            {
                true
            }
            TExprKind::Index {
                base,
                index,
                is_map,
                ..
            } => {
                !is_map
                    && (jit_list_record_type(&base.ty)
                        || matches!(
                            &base.ty,
                            Type::Apply { name, args }
                                if matches!(name.as_str(), "View" | "ComputeViewMut")
                                    && args.len() == 1
                                    && record_type_key(&args[0]).is_some()
                        ))
                    && matches!(&index.ty, Type::Int)
                    && resident_safe_expr(base, callees)
                    && resident_safe_expr(index, callees)
            }
            _ => resident_safe_expr(recv, callees),
        },
        TExprKind::MethodCall { recv, args, .. } => {
            resident_safe_expr(recv, callees)
                && args.iter().all(|a| resident_safe_call_arg(a, callees))
        }
        TExprKind::FnFieldCall { recv, args, .. } => {
            resident_safe_expr(recv, callees)
                && args.iter().all(|arg| resident_safe_call_arg(arg, callees))
        }
        TExprKind::StaticCall { args, .. } => {
            (!matches!(
                &expr.ty,
                Type::Apply { name, args }
                    if name == "Cell"
                        && args.first().is_some_and(|ty| !jit_cell_value_type(ty))
            )) && args.iter().all(|a| resident_safe_call_arg(a, callees))
        }
        TExprKind::AllocNew { .. } => true,
        TExprKind::PoolSlot { pool, id, .. } => {
            resident_safe_expr(pool, callees) && resident_safe_expr(id, callees)
        }
        TExprKind::IndexHook {
            base, index, ..
        } => {
            resident_safe_expr(base, callees) && resident_safe_expr(index, callees)
        }
        TExprKind::Drop(inner) | TExprKind::MaterializeView(inner) => {
            resident_safe_expr(inner, callees)
        }
        // Shadowed by `resident_safe_expr_work_item`; states the one law
        // (`resident_safe_raw_of` / `resident_safe_raw_deref`) so the two copies
        // cannot disagree. This arm answered a flat `false`, which refused every
        // audited pointer in the corpus although the lowering had covered a bare
        // local's own stack slot all along.
        TExprKind::RawOf(arg) => resident_safe_raw_of(arg) && resident_safe_expr(arg, callees),
        TExprKind::Deref(arg) => {
            resident_safe_raw_deref(&expr.ty) && resident_safe_expr(arg, callees)
        }
        TExprKind::EnumLit { payload, .. } => {
            jit_enum_type(&expr.ty) && resident_safe_enum_payload(payload, callees)
        }
        TExprKind::Present(inner) | TExprKind::Ok(inner) | TExprKind::Err(inner) => {
            resident_safe_expr(inner, callees)
        }
        TExprKind::Try { inner, convert, .. } => {
            matches!(
                convert,
                TIR::TTryConvert::None
                    | TIR::TTryConvert::DefaultErr
                    | TIR::TTryConvert::Typed(_)
                    | TIR::TTryConvert::WidenUnion { .. }
            ) && resident_safe_expr(inner, callees)
        }
        TExprKind::OptField { base, .. } => {
            matches!(
                &base.ty,
                Type::Option(payload)
                    if record_type_key(payload).is_some() || jit_tuple_type(payload)
            ) && resident_safe_expr(base, callees)
        }
        TExprKind::DecodeUnder { segment, inner } => {
            resident_safe_expr(segment, callees) && resident_safe_expr(inner, callees)
        }
        // Both zero-word literals, stated where the pair lives: `LowerCtx`
        // lowers `TExprKind::Absent` and `TExprKind::Unit` with the SAME
        // instruction (`lower_ctx.rs` `iconst(I64, 0)` for each). The gate
        // admitted only the first, so a fallible-void `fn run`'s bare `return`
        // — which TIR lowers to `Return(Some(Ok(Unit)))` under D-FAIL-EXIT1
        // (`lower/statements.rs` `Stmt::Return(None)`) — refused the whole
        // entry. `resident_safe_ct_value` already says "`CtValue::Unit` lowers
        // to the same zero word as `TExprKind::Unit`" and admits it; that made
        // one fact answer two ways depending on which carrier held it.
        TExprKind::Absent | TExprKind::Unit => true,
        TExprKind::DistinctCtor { arg, base, .. } => {
            jit_value_type(base) && resident_safe_expr(arg, callees)
        }
        // Moved file/codec handles (`output :: files.create(...)`, `^output` take).
        TExprKind::ResourceNew(inner) => resident_safe_expr(inner, callees),
        TExprKind::ResourceTake(_) => jit_value_type(&expr.ty),
        TExprKind::Close(_) => true,
        // List/Iter locals are marshalled by the resident string/print
        // lowerers. Keep this before the scalar-value gate so interpolation of
        // a materialized collection does not force an otherwise resident
        // collection program to tier0.
        TExprKind::Local(_)
            if jit_list_native_type(&expr.ty) || jit_list_iter_elem_type(&expr.ty).is_some() =>
        {
            true
        }
        _ if !jit_value_type(&expr.ty) => false,
        TExprKind::IntLit(_, _)
        | TExprKind::FloatLit(_)
        | TExprKind::BoolLit(_)
        | TExprKind::CharLit(_) => true,
        // One comptime table, asked with the slot this literal lands in
        // (`resident_safe_ct_value`). `lower_ctx.rs` lowers `CtLit` and `ConstRef`
        // through the one `lower_ct_value`, which reads the same slot.
        TExprKind::CtLit(value) => resident_safe_ct_value(value, Some(&expr.ty)),
        TExprKind::ConstRef(_) => true,
        TExprKind::StrLit(parts) => {
            resident_safe_string_parts(parts, callees)
        }
        TExprKind::Local(_) => true,
        TExprKind::Unary { op, operand } => {
            matches!(op, UnOp::Neg | UnOp::Not) && resident_safe_expr(operand, callees)
        }
        TExprKind::IncDec { ty, .. } => matches!(ty, Type::Int),
        TExprKind::Binary {
            op,
            overflow,
            lhs,
            rhs,
            ..
        } => {
            if matches!(op, BinOp::And | BinOp::Or) {
                return matches!(&lhs.ty, Type::Bool)
                    && matches!(&rhs.ty, Type::Bool)
                    && resident_safe_expr(lhs, callees)
                    && resident_safe_expr(rhs, callees);
            }
            // ProcessResult.signal is the one packed Option<Int> comparison
            // resident lowering supports; the JIT record holds the packed word
            // (`Process.rs` `alloc_process_result`). Key the admission on the
            // exact field shape, not its reported type. Other Option producers
            // use mixed packed/result-arena carriers, so do not admit them here.
            if is_packed_process_signal(lhs) || is_packed_process_signal(rhs) {
                return matches!(op, BinOp::Eq | BinOp::Ne)
                    && is_packed_process_signal(lhs)
                    && is_packed_process_signal(rhs)
                    && resident_safe_expr(lhs, callees)
                    && resident_safe_expr(rhs, callees);
            }
            // An Option operand: `LowerCtx::lower_binary` carries the
            // presence-then-payload equality row and the `Option: PartialOrd`
            // ordering row (auto-derived `Comparable` orders each field with
            // `<` then `>`, so an optional field arrives here as `T? < T?`).
            if matches!(&lhs.ty, Type::Option(_)) || matches!(&rhs.ty, Type::Option(_)) {
                return resident_safe_option_binary(op, &lhs.ty, &rhs.ty)
                    && resident_safe_expr(lhs, callees)
                    && resident_safe_expr(rhs, callees);
            }
            if *overflow {
                let lhs_int = intish_ty(&lhs.ty) || reactive_get_intish(lhs);
                let rhs_int = intish_ty(&rhs.ty) || reactive_get_intish(rhs);
                if !lhs_int || !rhs_int {
                    return false;
                }
            }
            resident_safe_expr(lhs, callees) && resident_safe_expr(rhs, callees)
        }
        TExprKind::CompareChain { operands, ops, hooks } => {
            // Same per-COMPARISON pairing as the work-item gate above (which
            // shadows this arm today): an op/hook belongs to the operand PAIR
            // it joins, never to a single operand, and the chain's last operand
            // is only ever the right-hand side of the final comparison.
            operands.len() == ops.len() + 1
                && hooks.len() == ops.len()
                && operands.iter().all(|e| resident_safe_expr(e, callees))
                && operands
                    .windows(2)
                    .zip(ops.iter().zip(hooks.iter()))
                    .all(|(pair, (op, hook))| {
                        matches!(op, BinOp::Lt | BinOp::Gt | BinOp::Le | BinOp::Ge)
                            && (*hook
                                || pair.iter().all(|operand| {
                                    matches!(
                                        &operand.ty,
                                        Type::Int
                                            | Type::IntN { .. }
                                            | Type::InlineRange { .. }
                                            | Type::Float
                                    )
                                }))
                    })
        }
        TExprKind::IfExpr {
            cond,
            then_body,
            then_value,
            else_body,
            else_value,
        } => {
            // Expression-position pattern conditions share statement lowering.
            let cond_ok = match cond.as_ref() {
                TIfCond::Plain(e) => {
                    matches!(&e.ty, Type::Bool) && resident_safe_expr(e, callees)
                }
                TIfCond::IfLet { pattern, subj } => {
                    matches!(
                        &pattern.pattern,
                        Pattern::Variant { .. } | Pattern::Ok { .. } | Pattern::Err { .. }
                    )
                        && resident_safe_expr(subj, callees)
                }
                TIfCond::Matches { subj, .. } => resident_safe_expr(subj, callees),
                TIfCond::IsNone { .. } | TIfCond::And { .. } => false,
            };
            cond_ok
                && then_body.iter().all(|s| resident_safe_stmt(s, callees))
                && resident_safe_expr(then_value, callees)
                && else_body.iter().all(|s| resident_safe_stmt(s, callees))
                && resident_safe_expr(else_value, callees)
        }
        TExprKind::Clone(inner) | TExprKind::ExplicitCopy(inner) => {
            resident_safe_expr(inner, callees)
        }
        TExprKind::Borrow { place, .. } => resident_safe_expr(place, callees),
        TExprKind::InlineBlock(stmts) => {
            (jit_value_type(&expr.ty)
                || jit_list_native_type(&expr.ty)
                || jit_struct_type(&expr.ty)
                || jit_tuple_type(&expr.ty))
                && stmts.iter().all(|stmt| resident_safe_stmt(stmt, callees))
        }
        TExprKind::Lambda(lam) => {
            lam.param_types.iter().all(jit_value_type)
                && lam.ret.as_ref().is_none_or(jit_value_type)
                && lam
                    .captures
                    .iter()
                    .all(|(_, _, ty)| jit_value_type(ty))
                && match &lam.executable {
                    TIR::TLambdaBody::Expr(expr) => resident_safe_expr(expr, callees),
                    TIR::TLambdaBody::Block(stmts) => {
                        stmts.iter().all(|stmt| resident_safe_stmt(stmt, callees))
                    }
                    TIR::TLambdaBody::SharedBlock(stmts) => {
                        stmts.iter().all(|stmt| resident_safe_stmt(stmt, callees))
                    }
                }
        }
        TExprKind::FnValue { kind } => match kind {
            TIR::TFnValueKind::NamedFn {
                name: Some(name), ..
            } => callees.contains(name),
            TIR::TFnValueKind::NamedFn {
                name: None,
                lambda: Some(lambda),
                ..
            } => {
                lambda.param_types.iter().all(jit_value_type)
                    && lambda.ret.as_ref().is_none_or(jit_value_type)
                    && lambda
                        .captures
                        .iter()
                        .all(|(_, _, ty)| jit_value_type(ty))
                    && match &lambda.executable {
                        TIR::TLambdaBody::Expr(expr) => resident_safe_expr(expr, callees),
                        TIR::TLambdaBody::Block(stmts) => {
                            stmts.iter().all(|stmt| resident_safe_stmt(stmt, callees))
                        }
                        TIR::TLambdaBody::SharedBlock(stmts) => {
                            stmts.iter().all(|stmt| resident_safe_stmt(stmt, callees))
                        }
                    }
            }
            TIR::TFnValueKind::NamedFn {
                name: None,
                lambda: None,
                ..
            } => false,
            TIR::TFnValueKind::Call { callee, args } => {
                resident_safe_expr(callee, callees)
                    && args.iter().all(|arg| resident_safe_call_arg(arg, callees))
            }
            TIR::TFnValueKind::Interrupt { value } => resident_safe_expr(value, callees),
            // D-STRUCT-POLICY1=A: a policy fn-value forwards the target's whole
            // argument contract through a generated checked wrapper. The resident
            // tier has no lowering for that wrapper yet, so decline it here and
            // let the canonical TIR evaluator run it. Declining preserves one
            // meaning across tiers; a resident lowering replaces this.
            TIR::TFnValueKind::Policy { .. } => false,
        },
        TExprKind::PatternMatches { subj, .. } => resident_safe_expr(subj, callees),
        TExprKind::OptionLift2 { f, a, b } => {
            let f_safe = match &f.kind {
                TExprKind::Lambda(lam) => {
                    lam.source_params.len() == 2
                        && lam.captures.iter().all(|(_, _, ty)| jit_value_type(ty))
                        && matches!(
                            &lam.executable,
                            TIR::TLambdaBody::Expr(e) if resident_safe_expr(e, callees)
                        )
                }
                _ => {
                    matches!(
                        &f.ty,
                        Type::Fn {
                            params,
                            ret: Some(ret),
                            ..
                        } if params.len() == 2
                            && params.iter().all(jit_value_type)
                            && jit_value_type(ret)
                    ) && resident_safe_expr(f, callees)
                }
            };
            f_safe && resident_safe_expr(a, callees)
                && resident_safe_expr(b, callees)
                && matches!(&expr.ty, Type::Option(inner) if jit_scalar_type(inner) || matches!(inner.as_ref(), Type::Named(_)|Type::Tuple(_)))
        }
        TExprKind::OverflowOpt {
            prefix,
            op,
            lhs,
            rhs,
        } => {
            matches!(prefix.as_str(), "wrapping" | "saturating" | "checked")
                && matches!(*op, "add" | "sub" | "mul" | "div" | "rem")
                && intish_ty(&lhs.ty)
                && intish_ty(&rhs.ty)
                && resident_safe_expr(lhs, callees)
                && resident_safe_expr(rhs, callees)
        }
        TExprKind::ClosureMethod { recv, op, args } => {
            resident_safe_closure_method(recv, op, args, callees)
        }
        TExprKind::TaskGroupAll { tasks } => {
            matches!(
                &expr.ty,
                Type::Result { ok, err }
                    if matches!(
                        err.as_ref(),
                        Type::Named(name) if name == jet_foundation::Syntax::TYPE_TASK_FAILURE
                    )
                        && jit_list_native_type(ok)
            ) && resident_safe_task_list_expr(tasks, callees)
        }
        TExprKind::TaskGroupRace { tasks } | TExprKind::TaskGroupAny { tasks } => {
            matches!(
                &expr.ty,
                Type::Result { ok, err }
                    if matches!(
                        ok.as_ref(),
                        ty if jit_value_type(ty)
                        && matches!(
                            err.as_ref(),
                            Type::Named(name) if name == jet_foundation::Syntax::TYPE_TASK_FAILURE
                        )
            )) && resident_safe_task_list_expr(tasks, callees)
        }
        TExprKind::SelectStart => true,
        TExprKind::SelectRecv { builder, channel } => {
            resident_safe_expr(builder, callees)
                && jit_concurrency_type(&channel.ty)
                && resident_safe_expr(channel, callees)
        }
        TExprKind::SelectAfter {
            builder,
            duration,
            value,
        } => {
            resident_safe_expr(builder, callees)
                && matches!(&duration.ty, Type::Named(name) if name == "Duration")
                && resident_safe_expr(duration, callees)
                && value.as_ref().map_or(true, |v| {
                    matches!(&v.ty, Type::Int) && resident_safe_expr(v, callees)
                })
        }
        TExprKind::SelectWait { builder, .. } => {
            jit_value_type(&expr.ty) && resident_safe_select_wait(builder, callees)
        }
        TExprKind::AmbientInput { prompt } => {
            prompt.as_ref().is_none_or(|p| resident_safe_expr(p, callees))
        }
        // D-OPTGC1: GcRead/GcEdit marshal through the collector-backed root
        // host calls; the lowering carries only the checked root handle.
        TExprKind::HostCall(host) => match host.as_ref() {
            THostCall::GcRead { .. } => true,
            THostCall::GcEdit {
                edit,
                index_temp,
                ..
            } => {
                resident_safe_expr(edit, callees)
                    && index_temp
                        .as_ref()
                        .is_none_or(|(_, e)| resident_safe_expr(e, callees))
            }
            THostCall::EnvSet { name, value, .. } => {
                resident_safe_expr(name, callees) && resident_safe_expr(value, callees)
            }
            THostCall::NumericBounds { ty, member } => {
                (jet_codegen::Comptime::MathLayout::integer_type_layout(ty).is_some()
                    && matches!(member.as_str(), "MIN" | "MAX"))
                    || (matches!(ty, Type::Float)
                        && matches!(member.as_str(), "INFINITY" | "NAN" | "EPSILON"))
            }
            THostCall::FixedListIndex { base, index, .. } => {
                resident_safe_expr(base, callees)
                    && resident_safe_expr(index, callees)
                    && (intish_ty(&index.ty) || matches!(&index.ty, Type::Named(_)))
            }
            THostCall::TupleIndex { base, .. } => resident_safe_expr(base, callees),
            THostCall::SwitchSubjectField { .. } | THostCall::SwitchSubjectValue => true,
            THostCall::StrMatchScan { .. } | THostCall::BinMatchScan { .. } => true,
            // Sema emits this node only after proving the receiver is a live
            // Cell guard and every projected path is valid and disjoint.
            THostCall::CellGuardProject { .. } => true,
            THostCall::CarrierFact { recv, .. } => {
                matches!(&recv.ty, Type::Result { .. }) && resident_safe_expr(recv, callees)
            }
            THostCall::Method {
                recv,
                method,
                args,
                ..
            } => {
                let method_supported = match &recv.ty {
                    Type::Apply { name, .. } if name == "Cell" => matches!(
                        (method.as_str(), args.len()),
                        ("get" | "guard_read" | "guard_edit", 0)
                            | ("set" | "replace" | "get_or_set" | "read" | "edit", 1)
                    ),
                    Type::Apply { name, .. } if name == "CellReadGuard" => matches!(
                        (method.as_str(), args.len()),
                        ("get", 0) | ("read", 1)
                    ),
                    Type::Apply { name, .. } if name == "CellEditGuard" => matches!(
                        (method.as_str(), args.len()),
                        ("get", 0) | ("set" | "read" | "edit", 1)
                    ),
                    Type::Shared(_) => matches!(
                        (method.as_str(), args.len()),
                        ("guard_read" | "guard_edit" | "downgrade" | "strong_count", 0)
                            | ("read" | "read_txn" | "edit" | "edit_txn", 1)
                    ),
                    Type::Apply { name, .. }
                        if name == jet_foundation::Syntax::TYPE_SHARED_WEAK =>
                    {
                        matches!((method.as_str(), args.len()), ("upgrade", 0))
                    }
                    _ => !matches!(method.as_str(), "guard_read" | "guard_edit"),
                };
                let cell_value_supported = match &recv.ty {
                    Type::Apply { name, args }
                        if matches!(
                            name.as_str(),
                            "Cell" | "CellReadGuard" | "CellEditGuard"
                        ) =>
                    {
                        args.first().is_some_and(jit_cell_value_type)
                    }
                    _ => true,
                };
                method_supported
                    && cell_value_supported
                    && resident_safe_expr(recv, callees)
                    && args.iter().all(|arg| resident_safe_expr(arg, callees))
            }
            THostCall::Helper { helper, args }
                if helper.ends_with("jet_std_clock_new") =>
            {
                args.len() == 1
                    && args.iter().all(|arg| match arg {
                        TIR::THostArg::Expr(expr) | TIR::THostArg::Borrow(expr) => {
                            resident_safe_expr(expr, callees)
                        }
                        TIR::THostArg::Lambda(_) => false,
                    })
            }
            THostCall::ExpiringValueNew {
                value,
                duration,
                clock,
            }
            | THostCall::ExpiringSecretNew {
                value,
                duration,
                clock,
                ..
            } => {
                resident_safe_expr(value, callees)
                    && resident_safe_expr(duration, callees)
                    && resident_safe_expr(clock, callees)
            }
            THostCall::YieldSend { value } => resident_safe_expr(value, callees),
            THostCall::TypedText { arg, .. } => resident_safe_expr(arg, callees),
            THostCall::TypedTextInterp { holes, .. } => {
                holes.iter().all(|h| resident_safe_expr(h, callees))
            }
            _ => false,
        },
        // Codable encode lowers to `JSONLit` (DataTree/JSON foreign enum).
        TExprKind::JSONLit { arg, .. } => match arg.as_ref() {
            None => true,
            Some(boxed) => {
                let (inner, _) = boxed.as_ref();
                enum_payload_value_type(&inner.ty) && resident_safe_expr(inner, callees)
            }
        },
        // D-DBDRIVER1: `DBValue.Int(n)` / `.Text(s)` / … — foreign prelude enum.
        TExprKind::DBValueLit { arg, .. } => match arg.as_ref() {
            None => true,
            Some(boxed) => {
                let (inner, _) = boxed.as_ref();
                enum_payload_value_type(&inner.ty) && resident_safe_expr(inner, callees)
            }
        },
        TExprKind::RequireStop { kind, .. } => match kind {
            TIR::TRequireKind::Require { cond, msg, .. } => {
                resident_safe_expr(cond, callees)
                    && msg
                        .as_ref()
                        .is_none_or(|m| resident_safe_expr(m, callees))
            }
            TIR::TRequireKind::RequireEq { left, right } => {
                resident_safe_expr(left, callees) && resident_safe_expr(right, callees)
            }
            TIR::TRequireKind::Panic { msg } => resident_safe_expr(msg, callees),
        },
        TExprKind::Todo { .. } => true,
        TExprKind::Unreachable { .. } => true,
        TExprKind::Uninit => matches!(&expr.ty, Type::FixedList { .. })
            && jit_list_native_type(&expr.ty),
        TExprKind::LayoutCompare { lhs, rhs, .. } => {
            resident_safe_expr(lhs, callees) && resident_safe_expr(rhs, callees)
        }
        TExprKind::LayoutLit { inner } => resident_safe_expr(inner, callees),
        TExprKind::MathBuiltin { args, .. } => {
            args.iter().all(|a| resident_safe_expr(a, callees))
        }
        TExprKind::MathLaneIndex { base, index, .. } => {
            resident_safe_expr(base, callees) && resident_safe_expr(index, callees)
        }
        TExprKind::MathSwizzleRead { recv, .. } => resident_safe_expr(recv, callees),
        // S58 / D-MEM-SENTRY1: `mem.Ptr<T>.from_addr(addr)` IS the address it is
        // given. An integer literal is the one operand with no live allocation
        // behind it — naming an address out of nothing is the whole point of
        // `examples/features/memory/unsafe_sentries_provenance.jet`, and the
        // shared Prelude sentry kernel owns its R0801. This tier pushes no
        // `jet_sentry_scope` and so can report nothing, so the shape stays off
        // it; `LowerCtx`'s `TExprKind::PtrFromAddr` arm still records the word as
        // a real address, because that is what a whole-program compile needs and
        // this refusal is what keeps any deref of it on the tier that answers.
        //
        // Every other operand keeps the provenance it already carried:
        // `from_addr(mem.address_of(local))` is real because `address_of`
        // recorded that very SSA value (`examples/features/lowlevel/mmio_board_write.jet`,
        // `examples/features/lowlevel/pointer_cast_deref.jet`).
        TExprKind::PtrFromAddr { addr, .. } => {
            !matches!(&addr.kind, TExprKind::IntLit(..)) && resident_safe_expr(addr, callees)
        }
        TExprKind::ExternCall { args, .. } => {
            args.iter().all(|a| resident_safe_expr(&a.value, callees))
        }
        TExprKind::SharedGuardValue { guard, .. }
        | TExprKind::SharedGuardMap { guard, .. } => resident_safe_expr(guard, callees),
        TExprKind::SharedGuardSplit { guard, .. } => resident_safe_expr(guard, callees),
        TExprKind::SharedGuardWait {
            guard,
            condition,
            predicate,
        } => {
            resident_safe_expr(guard, callees)
                && resident_safe_expr(condition, callees)
                && match &predicate.executable {
                    TIR::TLambdaBody::Expr(e) => resident_safe_expr(e, callees),
                    TIR::TLambdaBody::Block(stmts) => {
                        stmts.iter().all(|s| resident_safe_stmt(s, callees))
                    }
                    TIR::TLambdaBody::SharedBlock(stmts) => {
                        stmts.iter().all(|s| resident_safe_stmt(s, callees))
                    }
                }
        }
        TExprKind::ConditionNotify { condition, .. } => resident_safe_expr(condition, callees),

        _ => false,
    }
}

#[cfg(test)]
mod cell_value_type_tests {
    use super::jit_cell_value_type;
    use jet_codegen::AST::Type;

    fn int_map() -> Type {
        Type::Map {
            key: Box::new(Type::Int),
            key_span: None,
            value: Box::new(Type::Int),
        }
    }

    #[test]
    fn cell_resident_abi_rejects_non_string_maps_inside_wrappers() {
        assert!(!jit_cell_value_type(&int_map()));
        assert!(!jit_cell_value_type(&Type::Option(Box::new(int_map()))));
        assert!(!jit_cell_value_type(&Type::Result {
            ok: Box::new(int_map()),
            err: Box::new(Type::String),
        }));
    }
}

fn resident_safe_call_arg(arg: &TCallArg, callees: &HashSet<String>) -> bool {
    // Arc/fn-coerce still unsupported. borrow/mut_borrow pass heap handles;
    // clone lowers through `lower_clone` for the types below.
    if arg.arc_clone {
        return false;
    }
    // S48 trait-value slot — one law, see `resident_safe_trait_arg`.
    if !resident_safe_trait_arg(arg) {
        return false;
    }
    let ty = &arg.value.ty;
    let handle_pass = jit_value_type(ty)
        || jit_struct_type(ty)
        || jit_tuple_type(ty)
        || matches!(
            ty,
            Type::String
                | Type::List(_)
                | Type::FixedList { .. }
                | Type::Option(_)
                | Type::Map { .. }
        );
    if (arg.borrow || arg.mut_borrow) && !handle_pass {
        return false;
    }
    if arg.clone {
        let clone_ok = matches!(
            ty,
            Type::Int
                | Type::Float
                | Type::Bool
                | Type::Char
                | Type::String
                | Type::Option(_)
                | Type::IntN { .. }
                | Type::Float32
        ) || jit_struct_type(ty)
            || jit_compound_type(ty)
            || jit_tuple_type(ty)
            || jit_list_native_type(ty)
            || jit_list_record_type(ty)
            || jit_map_string_type(ty);
        if !clone_ok {
            return false;
        }
    }
    if arg.widen_to_vec && !(jit_list_native_type(ty) || matches!(ty, Type::FixedList { .. })) {
        return false;
    }
    resident_safe_expr(&arg.value, callees)
}

/// The single expression-bodied callback argument, taking `params` parameters,
/// or `None`.
///
/// `resident_safe_unary_lambda` is this with the body thrown away. Every arm
/// that also needs the body TYPE (`any`, `all`, `sort_by`, `sort_by` with a
/// comparator, `min_by`, `max_by`, `count_by`, `take_while`, `skip_while`,
/// `position`, `dedup_by`, `is_sorted_by`, `chunk_while`) used to call a
/// wrapper and then re-walk the same `args.first()` lambda through its own copy
/// of this ladder just to reach `body.ty` — six copies of one fact, and the
/// wrapper's `params` check was the only thing keeping them honest. One fact,
/// one walk (I8): the arity travels as `params` and the body comes back.
/// Mirrors `resident_safe_map_expr_callback`, which does the same for a map
/// callback at a given argument index.
fn resident_safe_expr_callback<'a>(
    args: &'a [TExpr],
    params: usize,
    callees: &HashSet<String>,
) -> Option<&'a TExpr> {
    if args.len() != 1 {
        return None;
    }
    let TExprKind::Lambda(lam) = &args[0].kind else {
        return None;
    };
    if !lam.prep.is_empty() || lam.source_params.len() != params {
        return None;
    }
    let TIR::TLambdaBody::Expr(body) = &lam.executable else {
        return None;
    };
    resident_safe_expr(body, callees).then_some(body.as_ref())
}

fn resident_safe_unary_lambda(args: &[TExpr], callees: &HashSet<String>) -> bool {
    resident_safe_expr_callback(args, 1, callees).is_some()
}

fn resident_safe_map_callback(args: &[TExpr], index: usize, callees: &HashSet<String>) -> bool {
    matches!(
        args.get(index).map(|arg| &arg.kind),
        Some(TExprKind::Lambda(lam))
            if lam.prep.is_empty()
                && lam.source_params.len() == 2
                && match &lam.executable {
                    TIR::TLambdaBody::Expr(body) => resident_safe_expr(body, callees),
                    TIR::TLambdaBody::Block(stmts) => {
                        stmts.iter().all(|stmt| resident_safe_stmt(stmt, callees))
                    }
                    TIR::TLambdaBody::SharedBlock(stmts) => {
                        stmts.iter().all(|stmt| resident_safe_stmt(stmt, callees))
                    }
                }
    )
}

fn resident_safe_map_expr_callback<'a>(
    args: &'a [TExpr],
    index: usize,
    callees: &HashSet<String>,
) -> Option<&'a TExpr> {
    let TExprKind::Lambda(lam) = &args.get(index)?.kind else {
        return None;
    };
    if !lam.prep.is_empty() || lam.source_params.len() != 2 {
        return None;
    }
    let TIR::TLambdaBody::Expr(body) = &lam.executable else {
        return None;
    };
    resident_safe_expr(body, callees).then_some(body.as_ref())
}

/// `each`/`each_mut`/`each_ref` take one callback of one parameter.
///
/// The body gate is `resident_safe_stmt` and nothing else. A block-bodied
/// callback used to carry a second, narrower statement table here
/// (`Let | Assign | ExprStmt`), which made both block arms UNSATISFIABLE: TIR
/// lowers a lambda block through `lower_stmts`
/// (`Codegen/TIR/lower/statements.rs`, `markers = true`), so the body always
/// opens with a `TStmt::SourceSpan` the second table refused. Every
/// block-bodied `each` therefore deopted the whole enclosing function —
/// `examples/features/collections/set.jet`'s
/// `s.each((n: Int) => { visited.push(n) })` was the corpus case (#1585). The
/// canonical predicate already answers `true` for both marker statements, and
/// the sibling callbacks (`resident_safe_map_callback`, the `EditDisjoint`
/// arm) never grew that second table.
///
/// The lowering pairs with this: `LowerCtx::lower_iter_each` hands the block to
/// `lower_collection_callback` → `functions_compile::lower_collection_callable_lambda`,
/// which runs `LowerCtx::lower_stmts` — the same statement lowering a resident
/// function body uses, markers included — and `sync_collection_captures` writes
/// the callback's captures back into the caller's slots.
fn resident_safe_each_lambda(args: &[TExpr], callees: &HashSet<String>) -> bool {
    args.len() == 1
        && matches!(
            &args[0].kind,
            TExprKind::Lambda(lam)
                if lam.prep.is_empty()
                    && lam.source_params.len() == 1
                    && match &lam.executable {
                        TIR::TLambdaBody::Expr(e) => resident_safe_expr(e, callees),
                        TIR::TLambdaBody::Block(stmts) => {
                            stmts.iter().all(|stmt| resident_safe_stmt(stmt, callees))
                        }
                        TIR::TLambdaBody::SharedBlock(stmts) => {
                            stmts.iter().all(|stmt| resident_safe_stmt(stmt, callees))
                        }
                    }
        )
}

fn resident_safe_fold_lambda(args: &[TExpr], callees: &HashSet<String>) -> bool {
    args.len() == 2
        && matches!(&args[0].ty, Type::Int)
        && resident_safe_expr(&args[0], callees)
        && matches!(
            &args[1].kind,
            TExprKind::Lambda(lam)
                if lam.prep.is_empty()
                    && lam.source_params.len() == 2
                    && matches!(
                        &lam.executable,
                        TIR::TLambdaBody::Expr(e) if resident_safe_expr(e, callees)
                    )
        )
}

/// `para_fold(seed, step, merge)` — seed is `() => U`, step/merge are binary.
fn resident_safe_para_fold_lambdas(args: &[TExpr], callees: &HashSet<String>) -> bool {
    if args.len() != 3 {
        return false;
    }
    let seed_ok = matches!(
        &args[0].kind,
        TExprKind::Lambda(lam)
            if lam.prep.is_empty()
                && lam.source_params.is_empty()
                && matches!(
                    &lam.executable,
                    TIR::TLambdaBody::Expr(e)
                        if matches!(&e.ty, Type::Int) && resident_safe_expr(e, callees)
                )
    );
    let bin_ok = |arg: &TExpr| {
        matches!(
            &arg.kind,
            TExprKind::Lambda(lam)
                if lam.prep.is_empty()
                    && lam.source_params.len() == 2
                    && matches!(
                        &lam.executable,
                        TIR::TLambdaBody::Expr(e) if resident_safe_expr(e, callees)
                    )
        )
    };
    seed_ok && bin_ok(&args[1]) && bin_ok(&args[2])
}

fn resident_safe_closure_method(
    recv: &TExpr,
    op: &TIR::TClosureOp,
    args: &[TExpr],
    callees: &HashSet<String>,
) -> bool {
    if !resident_safe_expr(recv, callees) {
        return false;
    }
    match op {
        TIR::TClosureOp::EditDisjoint => {
            let callback = args.get(1);
            args.len() == 2
                && resident_safe_expr(&args[0], callees)
                && matches!(
                    callback.map(|arg| &arg.kind),
                    Some(TExprKind::Lambda(lambda))
                        if lambda.prep.is_empty()
                            && lambda.source_params.len() == 2
                            && match &lambda.executable {
                                TIR::TLambdaBody::Expr(body) => resident_safe_expr(body, callees),
                                TIR::TLambdaBody::Block(stmts) => {
                                    stmts.iter().all(|stmt| resident_safe_stmt(stmt, callees))
                                }
                                TIR::TLambdaBody::SharedBlock(stmts) => {
                                    stmts.iter().all(|stmt| resident_safe_stmt(stmt, callees))
                                }
                            }
                )
        }
        TIR::TClosureOp::Any | TIR::TClosureOp::All => {
            jit_closure_elem_type_for(&recv.ty).is_some_and(|elem| {
                matches!(elem, Type::Int | Type::String | Type::Named(_))
            }) && resident_safe_expr_callback(args, 1, callees)
                .is_some_and(|body| matches!(&body.ty, Type::Bool))
        }
        TIR::TClosureOp::Map | TIR::TClosureOp::MapMut | TIR::TClosureOp::ViewMap => {
            jit_closure_elem_type_for(&recv.ty).is_some_and(|elem| {
                matches!(elem, Type::Int | Type::String | Type::Named(_))
            }) && resident_safe_unary_lambda(args, callees)
        }
        TIR::TClosureOp::ParaMap => {
            jit_closure_elem_type_for(&recv.ty).is_some_and(|elem| {
                matches!(elem, Type::Int | Type::String | Type::Named(_))
            }) && resident_safe_unary_lambda(args, callees)
        }
        // D-HOLE1: Option.map — packed Option ABI; unary lambda over the payload.
        TIR::TClosureOp::OptionMap => {
            matches!(
                &recv.ty,
                Type::Option(inner)
                    if jit_scalar_type(inner)
                        || matches!(inner.as_ref(), Type::Named(_) | Type::Tuple(_))
            ) && resident_safe_unary_lambda(args, callees)
        }
        TIR::TClosureOp::Filter | TIR::TClosureOp::ParaFilter => {
            jit_closure_elem_type_for(&recv.ty).is_some_and(|elem| {
                matches!(elem, Type::Int | Type::String | Type::Named(_))
            }) && resident_safe_unary_lambda(args, callees)
        }
        // `partition` and `para_partition` are one operation with two spellings:
        // both split into the same `(false_, true_)` tuple struct, in source
        // order, over the same field order (`resolve_closure_op` builds both
        // names from `[("false_", [T]), ("true_", [T])]`). The JIT lowers both
        // serially through `LowerCtx::lower_partition`, so they answer the same
        // predicate here.
        TIR::TClosureOp::Partition { .. } | TIR::TClosureOp::ParaPartition { .. } => {
            jit_closure_elem_type(&recv.ty).is_some_and(|elem| {
                matches!(elem, Type::Int | Type::String | Type::Named(_))
            }) && resident_safe_unary_lambda(args, callees)
        }
        TIR::TClosureOp::ParaFold => {
            jit_closure_elem_type(&recv.ty)
                .is_some_and(|elem| matches!(elem, Type::Int | Type::Named(_)))
                && args.len() == 3
                && resident_safe_para_fold_lambdas(args, callees)
        }
        TIR::TClosureOp::Each | TIR::TClosureOp::EachMut | TIR::TClosureOp::EachRef => {
            // `TraitObject` pairs with the same element filter in
            // `LowerCtx::lower_iter_each`: the walk hands the callback one i64
            // element handle, which is exactly a trait object's ABI.
            jit_closure_elem_type_for(&recv.ty).is_some_and(|elem| {
                matches!(
                    elem,
                    Type::Int | Type::String | Type::Named(_) | Type::TraitObject(_)
                )
            }) && resident_safe_each_lambda(args, callees)
        }
        TIR::TClosureOp::FilterMap => {
            matches!(jit_list_iter_elem_type(&recv.ty), Some(Type::String))
                && resident_safe_unary_lambda(args, callees)
        }
        TIR::TClosureOp::SortBy => {
            jit_closure_elem_type(&recv.ty)
                .is_some_and(|elem| matches!(elem, Type::String | Type::Named(_)))
                && resident_safe_expr_callback(args, 1, callees)
                    .is_some_and(|body| matches!(&body.ty, Type::Int))
        }
        TIR::TClosureOp::SortByDesc => {
            jit_closure_elem_type(&recv.ty)
                .is_some_and(|elem| matches!(elem, Type::String | Type::Named(_)))
                && resident_safe_expr_callback(args, 1, callees)
                    .is_some_and(|body| matches!(&body.ty, Type::Int | Type::String))
        }
        TIR::TClosureOp::SortByCompare => {
            jit_closure_elem_type(&recv.ty)
                .is_some_and(|elem| matches!(elem, Type::Int | Type::String | Type::Named(_)))
                && resident_safe_expr_callback(args, 2, callees).is_some_and(|body| {
                    matches!(&body.ty, Type::Named(name) if name == jet_foundation::Syntax::TYPE_ORDERING)
                })
        }
        // One index walk over `list_get` handles, so the element only has to be
        // one i64 the callback can bind — `Int` was the whole admitted set, and
        // a `[String]`/`[Shape]` receiver deopted its entire enclosing function
        // even though the loop never looks at the element itself.
        // The Bool requirement is new and NARROWS: `lower_iter_take_skip_while`
        // and `lower_iter_position` compare the callback result with `icmp` at
        // `I8`, so a non-Bool body would have handed Cranelift mismatched
        // operand types — an I2 internal compiler error, not a refusal. Sema
        // already types both predicates `Bool` (I3), so no live case is lost.
        // Lowering: `LowerCtx::lower_iter_take_skip_while` (TakeWhile/SkipWhile)
        // and `LowerCtx::lower_iter_position` (Position).
        TIR::TClosureOp::TakeWhile
        | TIR::TClosureOp::SkipWhile
        | TIR::TClosureOp::Position => {
            jit_closure_loop_elem(&recv.ty).is_some()
                && resident_safe_expr_callback(args, 1, callees)
                    .is_some_and(|body| matches!(&body.ty, Type::Bool))
        }
        TIR::TClosureOp::Fold | TIR::TClosureOp::Reduce | TIR::TClosureOp::ViewFold => {
            jit_closure_elem_type_for(&recv.ty)
                .or_else(|| jit_list_iter_elem_type(&recv.ty))
                .is_some_and(|elem| matches!(elem, Type::Int | Type::Named(_)))
                && resident_safe_fold_lambda(args, callees)
        }
        // The comparison is over the `Int` key the callback returns; the element
        // itself only travels as the packed-Option payload
        // (`LowerCtx::lower_iter_min_max_by`, `elem + 1` — the same one-based
        // carrier `pack_option_payload` writes), so any i64-shaped element
        // works. `String` was the whole admitted set, so `[Int].min_by(…)` and
        // `[Shape].max_by(…)` deopted their enclosing function.
        // Lowering: `LowerCtx::lower_iter_min_max_by`.
        TIR::TClosureOp::MinBy | TIR::TClosureOp::MaxBy => {
            jit_closure_loop_elem(&recv.ty).is_some()
                && resident_safe_expr_callback(args, 1, callees)
                    .is_some_and(|body| matches!(&body.ty, Type::Int))
        }
        TIR::TClosureOp::FlatMap => {
            let callback_returns_int_list = matches!(
                args.first().and_then(|arg| match &arg.kind {
                    TExprKind::Lambda(lambda) => match &lambda.executable {
                        TIR::TLambdaBody::Expr(body) => Some(&body.ty),
                        TIR::TLambdaBody::Block(_) | TIR::TLambdaBody::SharedBlock(_) => None,
                    },
                    _ => None,
                }),
                Some(Type::List(inner)) if matches!(inner.as_ref(), Type::Int)
            );
            (matches!(
                jit_closure_elem_type_for(&recv.ty),
                Some(Type::Int)
            ) || jit_list_of_int_list_type(&recv.ty))
                && callback_returns_int_list
                && resident_safe_unary_lambda(args, callees)
        }
        TIR::TClosureOp::DedupBy | TIR::TClosureOp::IsSortedBy => {
            matches!(jit_list_iter_elem_type(&recv.ty), Some(Type::Int))
                && resident_safe_expr_callback(args, 1, callees)
                    .is_some_and(|body| matches!(&body.ty, Type::Int))
        }
        TIR::TClosureOp::ChunkWhile => {
            matches!(jit_list_iter_elem_type(&recv.ty), Some(Type::Int))
                && resident_safe_expr_callback(args, 2, callees)
                    .is_some_and(|body| matches!(&body.ty, Type::Bool))
        }
        // `count_by` tallies into the one `Map<String, Int>` the JIT map ABI
        // carries, so the KEY the callback returns must be a `String`; the
        // element only has to be one i64 the callback can bind. This table used
        // to check the element and skip the key while
        // `LowerCtx::lower_iter_count_by` demanded both — a `[String]` receiver
        // with an `Int` key passed here and was refused there, so the function
        // deopted anyway. Both sides now state the same two facts.
        // Lowering: `LowerCtx::lower_iter_count_by`.
        TIR::TClosureOp::CountBy => {
            jit_closure_loop_elem(&recv.ty).is_some()
                && resident_safe_expr_callback(args, 1, callees)
                    .is_some_and(|body| matches!(&body.ty, Type::String))
        }
        TIR::TClosureOp::EachMap
        | TIR::TClosureOp::MapAny
        | TIR::TClosureOp::MapAll
        | TIR::TClosureOp::MapFilter
        | TIR::TClosureOp::MapMap
        | TIR::TClosureOp::MapFold
        | TIR::TClosureOp::MapFlatMap => {
            let map_ok = matches!(
                erase_runtime_qualifiers(&recv.ty),
                Type::Map { key, value, .. }
                    if matches!(key.as_ref(), Type::String)
                        && matches!(value.as_ref(), Type::Int)
            );
            if !map_ok {
                return false;
            }
            match op {
                TIR::TClosureOp::EachMap => resident_safe_map_callback(args, 0, callees),
                TIR::TClosureOp::MapAny | TIR::TClosureOp::MapAll => {
                    resident_safe_map_expr_callback(args, 0, callees)
                        .is_some_and(|body| matches!(&body.ty, Type::Bool))
                }
                TIR::TClosureOp::MapFilter => {
                    resident_safe_map_expr_callback(args, 0, callees)
                        .is_some_and(|body| matches!(&body.ty, Type::Bool))
                }
                TIR::TClosureOp::MapMap => {
                    resident_safe_map_expr_callback(args, 0, callees)
                        .is_some_and(|body| matches!(&body.ty, Type::Int))
                }
                TIR::TClosureOp::MapFold => {
                    args.len() == 2
                        && matches!(&args[0].ty, Type::Int)
                        && resident_safe_expr(&args[0], callees)
                        && resident_safe_map_expr_callback(args, 1, callees)
                            .is_some_and(|body| matches!(&body.ty, Type::Int))
                }
                TIR::TClosureOp::MapFlatMap => {
                    resident_safe_map_expr_callback(args, 0, callees).is_some_and(|body| {
                        matches!(
                            &body.ty,
                            Type::Map { key, value, .. }
                                if matches!(key.as_ref(), Type::String)
                                    && matches!(value.as_ref(), Type::Int)
                        )
                    })
                }
                _ => unreachable!("map closure arm checked above"),
            }
        }
        _ => false,
    }
}

fn resident_safe_enum_payload(payload: &TEnumPayload, callees: &HashSet<String>) -> bool {
    match payload {
        TEnumPayload::Unit => true,
        TEnumPayload::Positional(vals) => {
            vals.len() == 1
                && enum_payload_value_type(&vals[0].value.ty)
                && resident_safe_expr(&vals[0].value, callees)
        }
        TEnumPayload::Named(fields) => fields
            .iter()
            .all(|(_, a)| resident_safe_expr(&a.value, callees)),
    }
}

/// DataTree/JSON Object/Array payloads are Map/List handles; scalars stay as before.
fn enum_payload_value_type(ty: &Type) -> bool {
    matches!(
        ty,
        Type::Int
            | Type::Float
            | Type::Float32
            | Type::Bool
            | Type::String
            | Type::Char
            | Type::Named(_)
    ) || jit_map_string_type(ty)
        || jit_list_native_type(ty)
        || jit_list_record_type(ty)
        || matches!(ty, Type::List(elem) if matches!(elem.as_ref(), Type::Named(_)))
}

fn resident_safe_tuple_fields(fields: &[(String, TExpr)], callees: &HashSet<String>) -> bool {
    fields.iter().all(|(_, value)| {
        jit_value_type(&value.ty)
            && resident_safe_expr(value, callees)
    })
}

fn resident_safe_builtin_op(
    op: &TBuiltinOp,
    recv: &TExpr,
    args: &[TExpr],
    callees: &HashSet<String>,
) -> bool {
    // D-ZIPPAD1: zero columns is an empty `Iter<Unit>`, so the shape has no
    // receiver column at all. `lower_empty_zip_family` parks a `Unit`
    // placeholder in the receiver slot to keep `BuiltinMethod` well formed;
    // no tier reads it (`LowerCtx::lower_zip_family` guards every receiver and
    // column use behind `input_count > 0`, and it lowers to a plain `iconst 0`),
    // so walking it as an ordinary value-carrying receiver only deopts a shape
    // the JIT covers. The `Zip` arm below still proves the whole shape.
    let zero_column_zip = matches!(op, TBuiltinOp::Zip { input_count: 0, .. })
        && matches!(&recv.kind, TExprKind::Unit);
    if !zero_column_zip && !resident_safe_expr(recv, callees) {
        return false;
    }
    // D-TAINT1/D-TAG-SURFACE1: a `#Input`/other user fact tag on the receiver's
    // declared type (`Type::Tagged`) has no runtime representation — every
    // check below must dispatch on the real underlying type, the same
    // `erase_runtime_qualifiers` this file already uses for `intish_ty`, or a
    // tagged receiver misses its op arm here and the whole function wrongly
    // deopts to tier-0 interp even though the op is fully JIT-native-safe.
    let recv_ty = erase_runtime_qualifiers(&recv.ty);
    match op {
        // A `ProcessResult.output`/`.errors` receiver needs no special case: the
        // field read carries `Type::String`, the type sema declared. It used to
        // carry `Type::Int` (card 2021), which is why two of these arms — and
        // only two, so `child.output.replace(…)` never became resident at all —
        // used to admit the field structurally.
        TBuiltinOp::LenString => matches!(recv_ty, Type::String) && args.is_empty(),
        TBuiltinOp::Trim | TBuiltinOp::ToUpper | TBuiltinOp::ToLower => {
            matches!(recv_ty, Type::String) && args.is_empty()
        }
        TBuiltinOp::Replace => {
            matches!(recv_ty, Type::String)
                && args.len() == 2
                && args
                    .iter()
                    .all(|a| matches!(&a.ty, Type::String) && resident_safe_expr(a, callees))
        }
        TBuiltinOp::Push => {
            (jit_list_native_type(recv_ty)
                || matches!(recv_ty, Type::List(elem) if jit_value_type(elem))
                || matches!(recv_ty, Type::Apply { name, .. } if name == "PriorityQueue"))
                && args.len() == 1
                && (jit_value_type(&args[0].ty) || jit_compound_type(&args[0].ty))
                && resident_safe_expr(&args[0], callees)
        }
        TBuiltinOp::ListTryNew => args.is_empty(),
        TBuiltinOp::ListTryWithCapacity => {
            args.len() == 1
                && matches!(&args[0].ty, Type::Int)
                && resident_safe_expr(&args[0], callees)
        }
        TBuiltinOp::TryPush => {
            args.len() == 1
                && (jit_list_int_type(recv_ty) && matches!(&args[0].ty, Type::Int)
                    || jit_list_float_type(recv_ty) && matches!(&args[0].ty, Type::Float))
                && resident_safe_expr(&args[0], callees)
        }
        TBuiltinOp::TryReserve => {
            (jit_list_int_type(recv_ty) || jit_list_float_type(recv_ty))
                && args.len() == 1
                && matches!(&args[0].ty, Type::Int)
                && resident_safe_expr(&args[0], callees)
        }
        TBuiltinOp::TryInsertMap => {
            jit_map_string_int_type(recv_ty)
                && args.len() == 2
                && matches!(&args[0].ty, Type::String)
                && matches!(&args[1].ty, Type::Int)
                && resident_safe_expr(&args[0], callees)
                && resident_safe_expr(&args[1], callees)
        }
        TBuiltinOp::TryStringPush => {
            matches!(recv_ty, Type::String)
                && args.len() == 1
                && matches!(&args[0].ty, Type::String)
                && resident_safe_expr(&args[0], callees)
        }
        TBuiltinOp::Keys | TBuiltinOp::Values => {
            jit_map_resident_type(recv_ty) && args.is_empty()
        }
        TBuiltinOp::Sort => {
            (jit_list_int_type(recv_ty) || jit_list_string_type(recv_ty)) && args.is_empty()
        }
        TBuiltinOp::SortDesc => {
            (jit_list_int_type(recv_ty) || jit_list_string_type(recv_ty)) && args.is_empty()
        }
        TBuiltinOp::LenList => {
            (matches!(recv_ty, Type::String)
                || jit_list_native_type(recv_ty)
                || jit_list_iter_elem_type(recv_ty).is_some()
                || jit_closure_elem_type(recv_ty).is_some()
                || matches!(
                    recv_ty,
                    Type::List(inner) | Type::FixedList { elem: inner, .. }
                        if matches!(inner.as_ref(), Type::Named(name) if name == "Unit")
                )
                || jit_float_view_type(recv_ty)
                || jit_map_resident_type(recv_ty)
                || matches!(
                    recv_ty,
                    Type::Apply { name, .. }
                        if matches!(
                            name.as_str(),
                            "Set" | jet_foundation::Syntax::TYPE_QUEUE | jet_foundation::Syntax::TYPE_RANK | "PriorityQueue"
                        )
                )
                || matches!(recv_ty, Type::Named(name) if name == jet_foundation::Syntax::TYPE_BITS))
                && args.is_empty()
        }
        TBuiltinOp::GetList => {
            jit_list_native_type(recv_ty)
                && !jit_list_intn_type(recv_ty)
                && args.len() == 1
                && matches!(&args[0].ty, Type::Int)
                && resident_safe_expr(&args[0], callees)
        }
        TBuiltinOp::GetMap => {
            jit_map_resident_type(recv_ty)
                && args.len() == 1
                && (if jit_map_composite_key_type(recv_ty) {
                    jit_map_composite_key_expr(&args[0])
                } else if jit_map_int_type(recv_ty) {
                    jit_map_int_key_type(&args[0].ty)
                } else {
                    matches!(&args[0].ty, Type::String)
                })
                && resident_safe_expr(&args[0], callees)
        }
        TBuiltinOp::InsertMap | TBuiltinOp::AddNewMap => {
            jit_map_resident_type(recv_ty)
                && args.len() == 2
                && (if jit_map_composite_key_type(recv_ty) {
                    jit_map_composite_key_expr(&args[0])
                } else if jit_map_int_type(recv_ty) {
                    jit_map_int_key_type(&args[0].ty)
                } else {
                    matches!(&args[0].ty, Type::String)
                })
                && jit_value_type(&args[1].ty)
                && resident_safe_expr(&args[0], callees)
                && resident_safe_expr(&args[1], callees)
        }
        TBuiltinOp::MapMerge => {
            (jit_map_string_type(recv_ty) || jit_map_int_type(recv_ty))
                && args.len() == 1
                && (jit_map_string_type(&args[0].ty) || jit_map_int_type(&args[0].ty))
                && resident_safe_expr(&args[0], callees)
        }
        TBuiltinOp::MapMergeWith => {
            (jit_map_string_type(recv_ty) || jit_map_int_type(recv_ty))
                && args.len() == 2
                && (jit_map_string_type(&args[0].ty) || jit_map_int_type(&args[0].ty))
                && resident_safe_expr(&args[0], callees)
                && resident_safe_expr(&args[1], callees)
        }
        TBuiltinOp::RemoveMap => {
            (jit_map_string_int_type(recv_ty) || jit_map_composite_key_type(recv_ty))
                && args.len() == 1
                && (if jit_map_composite_key_type(recv_ty) {
                    jit_map_composite_key_expr(&args[0])
                } else {
                    matches!(&args[0].ty, Type::String)
                })
                && resident_safe_expr(&args[0], callees)
        }
        TBuiltinOp::MapCopy => {
            (jit_map_string_type(recv_ty) || jit_map_int_type(recv_ty)) && args.is_empty()
        }
        TBuiltinOp::MapFirst | TBuiltinOp::MapToList { .. } => {
            jit_map_string_int_type(recv_ty) && args.is_empty()
        }
        TBuiltinOp::MapEqual | TBuiltinOp::MapIntersection => {
            jit_map_string_int_type(recv_ty)
                && args.len() == 1
                && jit_map_string_int_type(&args[0].ty)
                && resident_safe_expr(&args[0], callees)
        }
        TBuiltinOp::MapMin | TBuiltinOp::MapMax => {
            jit_map_string_int_type(recv_ty) && args.is_empty()
        }
        TBuiltinOp::MapSliceKeys => {
            jit_map_string_int_type(recv_ty)
                && args.len() == 1
                && jit_list_string_type(&args[0].ty)
                && resident_safe_expr(&args[0], callees)
        }
        TBuiltinOp::MapNew => {
            matches!(recv_ty, Type::Map { .. }) && args.is_empty()
        }
        TBuiltinOp::MapFromKeys => {
            jit_list_string_type(recv_ty)
                && args.len() == 1
                && matches!(&args[0].ty, Type::Int)
                && resident_safe_expr(&args[0], callees)
        }
        TBuiltinOp::MapContainsValue => {
            jit_map_string_int_type(recv_ty)
                && args.len() == 1
                && matches!(&args[0].ty, Type::Int)
                && resident_safe_expr(&args[0], callees)
        }
        TBuiltinOp::MapPopFirst => {
            jit_map_string_int_type(recv_ty) && args.is_empty()
        }
        TBuiltinOp::JoinSep => {
            (jit_list_native_type(recv_ty) || jit_list_iter_elem_type(recv_ty).is_some())
                && args.len() == 1
                && matches!(&args[0].ty, Type::String)
                && resident_safe_expr(&args[0], callees)
        }
        TBuiltinOp::IsEmpty => {
            (jit_list_native_type(recv_ty)
                || jit_list_iter_elem_type(recv_ty).is_some()
                || jit_float_view_type(recv_ty)
                || jit_list_record_type(recv_ty)
                || jit_map_resident_type(recv_ty)
                || matches!(recv_ty, Type::List(elem) | Type::FixedList { elem, .. } if jit_value_type(elem))
                || matches!(
                    recv_ty,
                    Type::Apply { name, .. }
                        if matches!(
                            name.as_str(),
                            "Set" | jet_foundation::Syntax::TYPE_QUEUE | jet_foundation::Syntax::TYPE_RANK | "PriorityQueue"
                        )
                ))
                && args.is_empty()
        }
        TBuiltinOp::ParseInt | TBuiltinOp::ParseFloat => {
            matches!(recv_ty, Type::String) && args.is_empty()
        }
        TBuiltinOp::Slice { .. } => {
            (jit_list_int_type(recv_ty) || matches!(recv_ty, Type::String))
                && args.len() == 2
                && matches!(&args[0].ty, Type::Int)
                && matches!(&args[1].ty, Type::Int)
                && resident_safe_expr(&args[0], callees)
                && resident_safe_expr(&args[1], callees)
        }
        TBuiltinOp::Lines => {
            (matches!(recv_ty, Type::String)
                || matches!(recv_ty, Type::Named(n) if n == "FileReader"))
                && args.is_empty()
        }
        TBuiltinOp::Split => {
            matches!(recv_ty, Type::String)
                && args.len() == 1
                && matches!(&args[0].ty, Type::String)
                && resident_safe_expr(&args[0], callees)
        }
        TBuiltinOp::Chars | TBuiltinOp::Bytes => matches!(recv_ty, Type::String) && args.is_empty(),
        TBuiltinOp::After | TBuiltinOp::Before => {
            matches!(recv_ty, Type::String)
                && args.len() == 1
                && matches!(&args[0].ty, Type::String)
                && resident_safe_expr(&args[0], callees)
        }
        // JIT ABI: Iter producers already materialize list handles.
        TBuiltinOp::IterToList | TBuiltinOp::IterCollect | TBuiltinOp::ListLazy => {
            (jit_list_iter_elem_type(recv_ty).is_some()
                || jit_closure_elem_type(recv_ty).is_some()
                || jit_list_native_type(recv_ty)
                || matches!(
                    recv_ty,
                    Type::Apply { name, args }
                        if name == jet_foundation::Syntax::TYPE_ITER
                            && args.len() == 1
                            && matches!(&args[0], Type::Named(unit) if unit == "Unit")
                ))
                && args.is_empty()
        }
        TBuiltinOp::Take | TBuiltinOp::StepBy | TBuiltinOp::Chunks | TBuiltinOp::Windows => {
            jit_list_iter_elem_type(recv_ty).is_some()
                && args.len() == 1
                && matches!(&args[0].ty, Type::Int)
                && resident_safe_expr(&args[0], callees)
        }
        TBuiltinOp::Skip => {
            matches!(
                jit_list_iter_elem_type(recv_ty),
                Some(Type::Int | Type::Float | Type::String | Type::Char)
            ) && args.len() == 1
                && matches!(&args[0].ty, Type::Int)
                && resident_safe_expr(&args[0], callees)
        }
        TBuiltinOp::Dedup => jit_list_iter_elem_type(recv_ty).is_some() && args.is_empty(),
        TBuiltinOp::Sum { float: false } => {
            matches!(jit_list_iter_elem_type(recv_ty), Some(Type::Int)) && args.is_empty()
        }
        TBuiltinOp::Product { float: false }
        | TBuiltinOp::Min { float: false }
        | TBuiltinOp::Max { float: false } => {
            matches!(jit_list_iter_elem_type(recv_ty), Some(Type::Int)) && args.is_empty()
        }
        TBuiltinOp::Flatten => jit_list_of_int_list_type(recv_ty) && args.is_empty(),
        TBuiltinOp::Intersperse => {
            jit_list_iter_elem_type(recv_ty).is_some()
                && args.len() == 1
                && resident_safe_expr(&args[0], callees)
        }
        TBuiltinOp::Zip {
            input_count,
            fill_mode,
            field_types,
            flatten,
            ..
        } => {
            if *flatten {
                return false;
            }
            // Mirror `LowerCtx::lower_zip_family`'s own arity split. It reads
            // `input_count.saturating_sub(1)`, checks the same arity, then takes
            // the zero path; `checked_sub` bailing first made the zero case
            // below unreachable, so `zip()` deopted for a shape the JIT covers
            // (D-ZIPPAD1: zero columns is an empty `Iter<Unit>`, and the shared
            // policy `jet_zip_row_count` answers `Some(0)` rows for it).
            let input_args = (*input_count).saturating_sub(1);
            let fill_args = match fill_mode {
                TIR::TZipFillMode::DefaultNone => 0,
                TIR::TZipFillMode::Common | TIR::TZipFillMode::Columns => 1,
            };
            if args.len() != input_args + fill_args {
                return false;
            }
            if *input_count == 0 {
                return field_types.is_empty();
            }
            if field_types.len() != *input_count {
                return false;
            }
            let Some(recv_elem) = jit_zip_sequence_elem_type(recv_ty) else {
                return false;
            };
            if !jit_zip_elem_type(&recv_elem) {
                return false;
            }
            if *input_count == 1 {
                return jit_zip_field_type(&field_types[0])
                    && args.iter().all(|arg| resident_safe_expr(arg, callees));
            }
            if !args
                .iter()
                .take(input_args)
                .all(|arg| {
                    jit_zip_sequence_elem_type(&arg.ty)
                        .is_some_and(|elem| jit_zip_elem_type(&elem))
                })
            {
                return false;
            }
            if !args.iter().all(|arg| resident_safe_expr(arg, callees)) {
                return false;
            }
            if !field_types.iter().all(jit_zip_field_type) {
                return false;
            }
            true
        }
        // I8/I9, and the #2091 lift: admission is not a second policy.
        // `LowerCtx::unzip_column_kinds` is the ONE table naming which
        // `[(A, B)]` receivers `jet_jit_list_unzip` can honour and the column
        // kinds it needs; the `TBuiltinOp::Unzip` lowering reads it to pick the
        // immediates it hands the host, and this arm reads it to decide
        // residency. Consulting that table is the whole predicate.
        //
        // This used to be two hand-written halves and the split cost a run each
        // way. The gate demanded two `intish_ty` columns because the host read
        // BOTH fields with `record_get_int`, so a `record_set_string` column
        // read `None`, the row fell out of a `filter_map`, and
        // `["a","bb","c"].zip([1,2,3]).unzip()` answered two EMPTY lists — a
        // wrong answer, which is why narrowing the gate was right at the time.
        // Meanwhile the lowering accepted EVERY receiver, so the only thing
        // standing between that wrong answer and a user was this predicate. The
        // host now reads and republishes each column in its own kind
        // (`jit_zip_record_field` / `jit_zip_push_column_value`, the read and
        // write halves of `jit_zip_set_value`), so the honest set is every
        // column kind, and both sides get it from one place.
        TBuiltinOp::Unzip { .. } => {
            args.is_empty()
                && crate::lower_ctx::LowerCtx::unzip_column_kinds(recv_ty).is_some()
        }
        TBuiltinOp::TryCollect => {
            jit_result_list_elem(recv_ty).is_some() && args.is_empty()
        }
        // D-HOLE1: Option.zip — both sides packed Option; builds a Present pair.
        TBuiltinOp::OptionZip { .. } => {
            matches!(recv_ty, Type::Option(_))
                && args.len() == 1
                && matches!(&args[0].ty, Type::Option(_))
                && resident_safe_expr(&args[0], callees)
        }
        // D-RANGE-EXCL1=C: `indexes()` → Iter<Int> of every valid index.
        TBuiltinOp::Indexes => {
            (jit_list_native_type(recv_ty)
                || jit_list_iter_elem_type(recv_ty).is_some()
                || jit_closure_elem_type(recv_ty).is_some())
                && args.is_empty()
        }
        // JIT ABI: View/ViewMut materialize as owned list handles (inclusive slice).
        TBuiltinOp::ViewNew { .. } | TBuiltinOp::ViewMutNew { .. } => {
            (jit_list_native_type(recv_ty) || jit_list_record_type(recv_ty))
                && match args {
                    [range]
                        if matches!(&range.ty, Type::Named(name) if name == jet_foundation::Syntax::TYPE_RANGE) =>
                    {
                        resident_safe_expr(range, callees)
                    }
                    [start, end] => {
                        matches!(&start.ty, Type::Int)
                            && matches!(&end.ty, Type::Int)
                            && resident_safe_expr(start, callees)
                            && resident_safe_expr(end, callees)
                    }
                    _ => false,
                }
        }
        TBuiltinOp::ComputeViewNew { .. } | TBuiltinOp::ComputeViewMutNew { .. } => {
            recv_ty.is_compute_tensor_family()
                && match args {
                    [range]
                        if matches!(&range.ty, Type::Named(name) if name == jet_foundation::Syntax::TYPE_RANGE) =>
                    {
                        resident_safe_expr(range, callees)
                    }
                    [start, end] => {
                        matches!(&start.ty, Type::Int)
                            && matches!(&end.ty, Type::Int)
                            && resident_safe_expr(start, callees)
                            && resident_safe_expr(end, callees)
                    }
                    _ => false,
                }
        }
        TBuiltinOp::SplitWrite { .. } => {
            (jit_list_native_type(recv_ty) || jit_list_record_type(recv_ty))
                && args.len() == 1
                && matches!(&args[0].ty, Type::Int)
                && resident_safe_expr(&args[0], callees)
        }
        TBuiltinOp::GetDisjointWrite => {
            (jit_list_native_type(recv_ty) || jit_list_record_type(recv_ty))
                && args.len() == 1
                && jit_list_int_type(&args[0].ty)
                && resident_safe_expr(&args[0], callees)
        }
        // D-COLLBREADTH1=A: list.remove/count use the shared Prelude kernels;
        // the JIT only carries the resident Int-list handle and Option carrier.
        TBuiltinOp::RemoveList { mode, .. } => {
            matches!(mode, TIR::ListRemoveMode::Value | TIR::ListRemoveMode::Slot)
                && jit_list_int_type(recv_ty)
                && matches!(args.len(), 1 | 2)
                && matches!(&args[0].ty, Type::Int)
                && resident_safe_expr(&args[0], callees)
        }
        TBuiltinOp::CountList => {
            jit_list_int_type(recv_ty)
                && args.len() == 1
                && matches!(&args[0].ty, Type::Int)
                && resident_safe_expr(&args[0], callees)
        }
        TBuiltinOp::ExtendList | TBuiltinOp::ConcatList => false,
        TBuiltinOp::SetFrom => {
            (jit_list_int_type(recv_ty) || jit_list_string_type(recv_ty)) && args.is_empty()
        }
        TBuiltinOp::SetInsert | TBuiltinOp::SetRemove => {
            matches!(recv_ty, Type::Apply { name, args: targs }
                if name == "Set" && targs.len() == 1 && matches!(&targs[0], Type::Int | Type::String))
                && args.len() == 1
                && matches!(&args[0].ty, Type::Int | Type::String)
                && resident_safe_expr(&args[0], callees)
        }
        TBuiltinOp::SetToList => {
            matches!(recv_ty, Type::Apply { name, args: targs }
                if name == "Set" && targs.len() == 1 && matches!(&targs[0], Type::Int | Type::String))
                && args.is_empty()
        }
        TBuiltinOp::SetSort | TBuiltinOp::SetShuffle => {
            matches!(recv_ty, Type::Apply { name, args: targs }
                if name == "Set" && targs.len() == 1 && matches!(&targs[0], Type::Int | Type::String))
                && args.is_empty()
        }
        TBuiltinOp::SetCopy | TBuiltinOp::SetCapacity | TBuiltinOp::SetFirst | TBuiltinOp::SetValues => {
            matches!(recv_ty, Type::Apply { name, args: targs }
                if name == "Set" && targs.len() == 1 && matches!(&targs[0], Type::Int | Type::String))
                && args.is_empty()
        }
        TBuiltinOp::SetPop => {
            matches!(recv_ty, Type::Apply { name, args: targs }
                if name == "Set" && targs.len() == 1 && matches!(&targs[0], Type::Int | Type::String))
                && args.len() == 1
                && matches!(&args[0].ty, Type::Int | Type::String)
                && resident_safe_expr(&args[0], callees)
        }
        TBuiltinOp::SetEqual => {
            matches!(recv_ty, Type::Apply { name, args: targs }
                if name == "Set" && targs.len() == 1 && matches!(&targs[0], Type::Int | Type::String))
                && args.len() == 1
                && matches!(&args[0].ty, Type::Apply { name, args: targs }
                    if name == "Set" && targs.len() == 1 && matches!(&targs[0], Type::Int | Type::String))
                && resident_safe_expr(&args[0], callees)
        }
        TBuiltinOp::SetUnion
        | TBuiltinOp::SetIntersection
        | TBuiltinOp::SetDifference
        | TBuiltinOp::SetSymmetricDifference
        | TBuiltinOp::SetIsSubset
        | TBuiltinOp::SetIsSuperset
        | TBuiltinOp::SetIsDisjoint => {
            matches!(recv_ty, Type::Apply { name, args: targs }
                if name == "Set" && targs.len() == 1 && matches!(&targs[0], Type::Int | Type::String))
                && args.len() == 1
                && matches!(&args[0].ty, Type::Apply { name, args: targs }
                    if name == "Set" && targs.len() == 1 && matches!(&targs[0], Type::Int | Type::String))
                && resident_safe_expr(&args[0], callees)
        }
        TBuiltinOp::SortedSetFrom => {
            (jit_list_int_type(recv_ty) || jit_list_string_type(recv_ty)) && args.is_empty()
        }
        TBuiltinOp::SortedSetInsert | TBuiltinOp::SortedSetRemove => {
            matches!(recv_ty, Type::Apply { name, args: targs }
                if name == jet_foundation::Syntax::TYPE_RANK && targs.len() == 1 && matches!(&targs[0], Type::Int | Type::String))
                && args.len() == 1
                && matches!(&args[0].ty, Type::Int | Type::String)
                && resident_safe_expr(&args[0], callees)
        }
        TBuiltinOp::SortedSetUnion
        | TBuiltinOp::SortedSetIntersection
        | TBuiltinOp::SortedSetDifference
        | TBuiltinOp::SortedSetSymmetricDifference
        | TBuiltinOp::SortedSetIsSubset
        | TBuiltinOp::SortedSetIsSuperset
        | TBuiltinOp::SortedSetIsDisjoint => {
            matches!(recv_ty, Type::Apply { name, args: targs }
                if name == jet_foundation::Syntax::TYPE_RANK && targs.len() == 1 && matches!(&targs[0], Type::Int | Type::String))
                && args.len() == 1
                && matches!(&args[0].ty, Type::Apply { name, args: targs }
                    if name == jet_foundation::Syntax::TYPE_RANK && targs.len() == 1 && matches!(&targs[0], Type::Int | Type::String))
                && resident_safe_expr(&args[0], callees)
        }
        TBuiltinOp::PriorityQueueFrom => {
            jit_list_int_type(recv_ty) && args.is_empty()
        }
        TBuiltinOp::SortedSetToList
        | TBuiltinOp::PriorityQueuePeek
        | TBuiltinOp::PriorityQueueToSortedList
        | TBuiltinOp::LruKeys
        | TBuiltinOp::BitSetCount
        | TBuiltinOp::BitSetToList
        | TBuiltinOp::BitSetCopy
        | TBuiltinOp::ByteBufferToBytes => args.is_empty(),
        TBuiltinOp::First => {
            (matches!(recv_ty, Type::Apply { name, .. } if name == jet_foundation::Syntax::TYPE_RANK)
                || matches!(
                    jit_list_iter_elem_type(recv_ty),
                    Some(Type::Int | Type::Float | Type::String | Type::Char)
                ))
                && args.is_empty()
        }
        TBuiltinOp::Last => {
            (matches!(recv_ty, Type::Apply { name, .. } if name == jet_foundation::Syntax::TYPE_RANK)
                || matches!(recv_ty, Type::List(_) | Type::FixedList { .. })
                || jit_list_native_type(recv_ty))
                && args.is_empty()
        }
        // `pq.pop()` resolves to `PriorityQueuePop`, not `Pop`, since
        // e7fdc84a5 split the verb (`resolve_builtin_op`, TIR
        // `lower/builtins.rs`). Lowering kept the pair together —
        // `LowerCtx::lower_builtin_method_dispatch` matches
        // `TBuiltinOp::Pop | TBuiltinOp::PriorityQueuePop` and routes a
        // `PriorityQueue` receiver to `priority_queue_pop`
        // (`lower_ctx.rs`) — but this predicate did not, so the
        // `PriorityQueue` receiver named on the next line became
        // unreachable and every `pq.pop()` fell to the `_ => false` floor.
        TBuiltinOp::Pop | TBuiltinOp::PriorityQueuePop => {
            (matches!(recv_ty, Type::Apply { name, .. } if name == "PriorityQueue")
                || jit_list_native_type(recv_ty)
                || matches!(recv_ty, Type::List(elem) if jit_value_type(elem) || matches!(elem.as_ref(), Type::Apply { name, .. } if name == "Task")))
                && args.is_empty()
        }
        TBuiltinOp::LruPut | TBuiltinOp::LruAddNew => {
            matches!(recv_ty, Type::Apply { name, .. } if name == "Cache")
                && args.len() == 2
                && args.iter().all(|arg| resident_safe_expr(arg, callees))
        }
        TBuiltinOp::LruGet | TBuiltinOp::ContainsKey => {
            matches!(recv_ty, Type::Apply { name, .. } if name == "Cache")
                && args.len() == 1
                && resident_safe_expr(&args[0], callees)
        }
        TBuiltinOp::BitSetAdd | TBuiltinOp::BitSetRemove => {
            matches!(recv_ty, Type::Named(name) if name == jet_foundation::Syntax::TYPE_BITS)
                && args.len() == 1
                && resident_safe_expr(&args[0], callees)
        }
        TBuiltinOp::BitSetNew | TBuiltinOp::ByteBufferNew => args.is_empty(),
        TBuiltinOp::ByteBufferWithCapacity => {
            matches!(recv_ty, Type::Int) && args.is_empty() && resident_safe_expr(recv, callees)
        }
        TBuiltinOp::ByteBufferFrom => {
            matches!(recv_ty, Type::List(_))
                && args.is_empty()
                && resident_safe_expr(recv, callees)
        }
        TBuiltinOp::ByteBufferWrite { .. } => {
            matches!(recv_ty, Type::Named(name) if name == jet_foundation::Syntax::TYPE_BYTES)
                && args.len() == 1
                && resident_safe_expr(&args[0], callees)
        }
        TBuiltinOp::ByteBufferMethod { .. } => {
            matches!(recv_ty, Type::Named(name) if name == jet_foundation::Syntax::TYPE_BYTES)
                && args.iter().all(|arg| resident_safe_expr(arg, callees))
        }
        TBuiltinOp::TrimView => matches!(recv_ty, Type::String) && args.is_empty(),
        TBuiltinOp::AfterView | TBuiltinOp::BeforeView => {
            matches!(recv_ty, Type::String)
                && args.len() == 1
                && resident_safe_expr(&args[0], callees)
        }
        TBuiltinOp::BagAdd | TBuiltinOp::BagRemove | TBuiltinOp::BagHas | TBuiltinOp::BagCount => {
            matches!(recv_ty, Type::Apply { name, args: targs }
                if name == jet_foundation::Syntax::TYPE_TALLY && targs.len() == 1 && jit_bag_raw_key_candidate(&targs[0]))
                && args.len() == 1
                && resident_safe_expr(&args[0], callees)
        }
        TBuiltinOp::BagLen => {
            matches!(recv_ty, Type::Apply { name, args: targs }
                if name == jet_foundation::Syntax::TYPE_TALLY && targs.len() == 1 && jit_bag_raw_key_candidate(&targs[0]))
                && args.is_empty()
        }
        TBuiltinOp::Contains
            if matches!(recv_ty, Type::Named(name) if name == jet_foundation::Syntax::TYPE_RANGE) =>
        {
            args.len() == 1
                && matches!(&args[0].ty, Type::Int)
                && resident_safe_expr(&args[0], callees)
        }
        // `Bits.has(bit)` — `lower_builtin_method_dispatch` routes this receiver
        // to the shared `bit_set_has` membership kernel.
        TBuiltinOp::Contains
            if matches!(recv_ty, Type::Named(name) if name == jet_foundation::Syntax::TYPE_BITS) =>
        {
            args.len() == 1
                && matches!(&args[0].ty, Type::Int)
                && resident_safe_expr(&args[0], callees)
        }
        TBuiltinOp::Contains
            if matches!(recv_ty, Type::Apply { name, args: targs }
                if matches!(name.as_str(), "Set" | jet_foundation::Syntax::TYPE_RANK)
                    && targs.len() == 1
                    && matches!(&targs[0], Type::Int | Type::String)) =>
        {
            args.len() == 1
                && match (recv_ty, &args[0].ty) {
                    (Type::Apply { args: targs, .. }, Type::Int) => {
                        matches!(targs.as_slice(), [Type::Int])
                    }
                    (Type::Apply { args: targs, .. }, Type::String) => {
                        matches!(targs.as_slice(), [Type::String])
                    }
                    _ => false,
                }
                && resident_safe_expr(&args[0], callees)
        }
        TBuiltinOp::Contains
            if matches!(recv_ty, Type::List(inner) if **inner == Type::String) =>
        {
            args.len() == 1
                && matches!(&args[0].ty, Type::String)
                && resident_safe_expr(&args[0], callees)
        }
        TBuiltinOp::Contains => {
            matches!(recv_ty, Type::String)
                && args.len() == 1
                && matches!(&args[0].ty, Type::String)
                && resident_safe_expr(&args[0], callees)
        }
        TBuiltinOp::StartsWith | TBuiltinOp::EndsWith => {
            (matches!(recv_ty, Type::String)
                && args.len() == 1
                && matches!(&args[0].ty, Type::String)
                || jit_list_int_type(recv_ty)
                    && args.len() == 1
                    && jit_list_int_type(&args[0].ty))
                && resident_safe_expr(&args[0], callees)
        }
        TBuiltinOp::IterRepeat | TBuiltinOp::IterCycle | TBuiltinOp::IterDropLast => {
            matches!(jit_list_iter_elem_type(recv_ty), Some(Type::Int))
                && args.len() == 1
                && matches!(&args[0].ty, Type::Int)
                && resident_safe_expr(&args[0], callees)
        }
        TBuiltinOp::IterShuffle | TBuiltinOp::IterIsSorted => {
            matches!(jit_list_iter_elem_type(recv_ty), Some(Type::Int)) && args.is_empty()
        }
        TBuiltinOp::IterLastIndexOf => {
            matches!(jit_list_iter_elem_type(recv_ty), Some(Type::Int))
                && args.len() == 1
                && matches!(&args[0].ty, Type::Int)
                && resident_safe_expr(&args[0], callees)
        }
        TBuiltinOp::IterAverage { float: false } => {
            matches!(jit_list_iter_elem_type(recv_ty), Some(Type::Int)) && args.is_empty()
        }
        TBuiltinOp::IterAverage { float: true } => {
            matches!(jit_list_iter_elem_type(recv_ty), Some(Type::Float)) && args.is_empty()
        }
        TBuiltinOp::IterCompare => {
            matches!(jit_list_iter_elem_type(recv_ty), Some(Type::Int))
                && args.len() == 1
                && jit_list_int_type(&args[0].ty)
                && resident_safe_expr(&args[0], callees)
        }
        TBuiltinOp::IterSplit { .. } => {
            matches!(jit_list_iter_elem_type(recv_ty), Some(Type::Int))
                && args.len() == 1
                && matches!(&args[0].ty, Type::Int)
                && resident_safe_expr(&args[0], callees)
        }
        TBuiltinOp::ListSlice => {
            jit_list_int_type(recv_ty)
                && args.len() == 2
                && matches!(&args[0].ty, Type::Int)
                && matches!(&args[1].ty, Type::Int)
                && args.iter().all(|arg| resident_safe_expr(arg, callees))
        }
        TBuiltinOp::ListCopy => jit_list_int_type(recv_ty) && args.is_empty(),
        TBuiltinOp::ListEqual => {
            jit_list_int_type(recv_ty)
                && args.len() == 1
                && jit_list_int_type(&args[0].ty)
                && resident_safe_expr(&args[0], callees)
        }
        TBuiltinOp::ListBinarySearch => {
            jit_list_int_type(recv_ty)
                && args.len() == 1
                && matches!(&args[0].ty, Type::Int)
                && resident_safe_expr(&args[0], callees)
        }
        TBuiltinOp::ListUnion | TBuiltinOp::ListIntersection | TBuiltinOp::ListDifference => {
            jit_list_int_type(recv_ty)
                && args.len() == 1
                && jit_list_int_type(&args[0].ty)
                && resident_safe_expr(&args[0], callees)
        }
        TBuiltinOp::ListRandom | TBuiltinOp::ListMinMax { .. } => {
            jit_list_int_type(recv_ty) && args.is_empty()
        }
        TBuiltinOp::ListReplace => {
            jit_list_int_type(recv_ty)
                && args.len() == 2
                && matches!(&args[0].ty, Type::Int)
                && matches!(&args[1].ty, Type::Int)
                && args.iter().all(|arg| resident_safe_expr(arg, callees))
        }
        TBuiltinOp::PriorityQueueRemove { mode, .. } => {
            matches!(
                mode,
                TIR::ListRemoveMode::Value | TIR::ListRemoveMode::Slot
            )
                && matches!(
                    recv_ty,
                    Type::Apply { name, args: targs }
                        if name == "PriorityQueue"
                            && matches!(targs.as_slice(), [Type::Int])
                )
                // D-LISTREMOVE1/F: the selector rides as an optional SECOND
                // argument, so `pq.remove(0, .Slot)` arrives with two. The
                // sibling `RemoveList` arm above already spells this `1 | 2`,
                // and `mode` is read from the selector during lowering, which
                // only ever evaluates `args[0]`. Demanding exactly one argument
                // judged every `.Slot` call non-resident-safe while
                // `try_compile_bundle` compiled it, so the scanner and the
                // lowering disagreed about one construct (AGENTS.md I8).
                && matches!(args.len(), 1 | 2)
                && matches!(&args[0].ty, Type::Int)
                && resident_safe_expr(&args[0], callees)
        }
        TBuiltinOp::MatchGroup => {
            (matches!(recv_ty, Type::Named(n) if n == "Match")
                || matches!(&recv.kind, TExprKind::Local(_)))
                && args.len() == 1
                && resident_safe_expr(&args[0], callees)
        }
        TBuiltinOp::DequePushFront | TBuiltinOp::DequePushBack => {
            matches!(recv_ty, Type::Apply { name, args: targs }
                if name == jet_foundation::Syntax::TYPE_QUEUE && targs.len() == 1 && matches!(&targs[0], Type::Int))
                && args.len() == 1
                && matches!(&args[0].ty, Type::Int)
                && resident_safe_expr(&args[0], callees)
        }
        TBuiltinOp::DequePopFront
        | TBuiltinOp::DequePopBack
        | TBuiltinOp::DequePeekFront
        | TBuiltinOp::DequePeekBack
        | TBuiltinOp::DequeCapacity
        | TBuiltinOp::DequeToList
        | TBuiltinOp::DequeReverse => {
            matches!(recv_ty, Type::Apply { name, args: targs }
                if name == jet_foundation::Syntax::TYPE_QUEUE && targs.len() == 1 && matches!(&targs[0], Type::Int))
                && args.is_empty()
        }
        TBuiltinOp::DequeContains | TBuiltinOp::DequeDelete => {
            matches!(recv_ty, Type::Apply { name, args: targs }
                if name == jet_foundation::Syntax::TYPE_QUEUE && targs.len() == 1 && matches!(&targs[0], Type::Int))
                && args.len() == 1
                && matches!(&args[0].ty, Type::Int)
                && resident_safe_expr(&args[0], callees)
        }
        TBuiltinOp::DequeGet | TBuiltinOp::DequeSplit => {
            matches!(recv_ty, Type::Apply { name, args: targs }
                if name == jet_foundation::Syntax::TYPE_QUEUE && targs.len() == 1 && matches!(&targs[0], Type::Int))
                && args.len() == 1
                && matches!(&args[0].ty, Type::Int)
                && resident_safe_expr(&args[0], callees)
        }
        TBuiltinOp::DequeJoin => {
            matches!(recv_ty, Type::Apply { name, args: targs }
                if name == jet_foundation::Syntax::TYPE_QUEUE && targs.len() == 1 && matches!(&targs[0], Type::Int))
                && args.len() == 1
                && matches!(&args[0].ty, Type::String)
                && resident_safe_expr(&args[0], callees)
        }
        TBuiltinOp::DequeFrom => {
            matches!(
                recv_ty,
                Type::List(inner) if matches!(inner.as_ref(), Type::Int)
            ) && args.is_empty()
        }
        TBuiltinOp::InsertList => {
            jit_list_int_type(recv_ty)
                && args.len() == 2
                && matches!(&args[0].ty, Type::Int)
                && matches!(&args[1].ty, Type::Int)
                && resident_safe_expr(&args[0], callees)
                && resident_safe_expr(&args[1], callees)
        }
        // #1476 / #1409 ambient String surface — keep examples resident (I9).
        TBuiltinOp::TrimStart | TBuiltinOp::TrimEnd | TBuiltinOp::StringToTitle => {
            matches!(recv_ty, Type::String) && args.is_empty()
        }
        TBuiltinOp::PadStart | TBuiltinOp::PadEnd => {
            matches!(recv_ty, Type::String)
                && args.len() == 2
                && matches!(&args[0].ty, Type::Int)
                && matches!(&args[1].ty, Type::String)
                && args.iter().all(|a| resident_safe_expr(a, callees))
        }
        TBuiltinOp::StringIndexOf | TBuiltinOp::StringCount => {
            matches!(recv_ty, Type::String)
                && args.len() == 1
                && matches!(&args[0].ty, Type::String)
                && resident_safe_expr(&args[0], callees)
        }
        TBuiltinOp::StringIsAlphabetic
        | TBuiltinOp::StringIsNumeric
        | TBuiltinOp::StringIsWhitespace
        | TBuiltinOp::StringIsAscii => matches!(recv_ty, Type::String) && args.is_empty(),
        TBuiltinOp::StringSplitOnce { .. } => {
            matches!(recv_ty, Type::String)
                && args.len() == 1
                && matches!(&args[0].ty, Type::String)
                && resident_safe_expr(&args[0], callees)
        }
        TBuiltinOp::StringMethod { method } => {
            matches!(recv_ty, Type::String)
                && match method.as_str() {
                    "is_lower" | "is_upper" | "capitalize" | "swapcase" | "copy" | "reverse"
                    | "normalize" => args.is_empty(),
                    "last_index_of" | "remove_prefix" | "remove_suffix" | "compare" | "equal"
                    | "rsplit" => {
                        args.len() == 1
                            && matches!(&args[0].ty, Type::String)
                            && resident_safe_expr(&args[0], callees)
                    }
                    _ => false,
                }
        }
        _ => false,
    }
}

pub(crate) fn resident_safe_stmt(stmt: &TStmt, callees: &HashSet<String>) -> bool {
    match stmt {
        TStmt::Contract { contract } => {
            matches!(
                contract.disposition,
                TIR::TContractDisposition::Proven | TIR::TContractDisposition::Stripped
            ) || (resident_safe_expr(&contract.condition, callees)
                && resident_safe_expr(&contract.message, callees))
        }
        TStmt::ContractScope {
            pre,
            body,
            post,
            ..
        } => {
            pre.iter().chain(post).all(|contract| {
                matches!(
                    contract.disposition,
                    TIR::TContractDisposition::Proven | TIR::TContractDisposition::Stripped
                ) || (resident_safe_expr(&contract.condition, callees)
                    && resident_safe_expr(&contract.message, callees))
            }) && body.iter().all(|stmt| resident_safe_stmt(stmt, callees))
        }
        TStmt::LineMarker(_) | TStmt::SourceSpan(_) => true,
        TStmt::Let { init, gc_promotion: _, gc_transferred: _, .. } => {
            // Promotion/transfer crosses the collector-backed root ABI. The
            // host owns root identity; this tier only lowers the checked value.
            resident_safe_expr(init, callees)
        }
        // D-TUPLE-DESTRUCT1: `(tx, rx) := tasks.channel<T>()` — the one
        // tuple-destructure shape this tier covers (general `TupleDestructure` /
        // `StructDestructure` / `ListDestructure` are not covered at all otherwise;
        // that's unrelated pre-existing scope, not narrowed here). Mirrors the old
        // single-handle `let ch := tasks.channel()` + `ch.sender()` shape exactly:
        // one `channel_new` call for the receiver handle, one `channel_sender` call
        // on it for the sender handle — same two host calls, now both fired at the
        // producer site instead of at a later `.sender()` call.
        TStmt::TupleDestructure { init, binds, .. } => {
            (binds.len() == 2
                && matches!(
                    &init.kind,
                    TExprKind::CoreCall { module, method, args, .. }
                        if module == "core.tasks"
                            && method == "channel"
                            && args.len() <= 1
                            && args.iter().all(|a| resident_safe_expr(a, callees))
                ))
                || (jit_tuple_type(&init.ty)
                    && binds.len()
                        == match &init.ty {
                            Type::Tuple(fields) => fields.len(),
                            _ => 0,
                        }
                    && resident_safe_expr(init, callees))
                || (matches!(
                    &init.ty,
                    Type::Apply { name, .. } if name == "VjpRun"
                ) && binds.len() <= 3
                    && !binds.is_empty()
                    && resident_safe_expr(init, callees))
                || (matches!(
                    &init.ty,
                    Type::Tuple(fields)
                        if fields.len() == binds.len()
                            && fields.iter().all(|(_, ty)| matches!(
                                ty.as_ref(),
                                Type::Apply { name, .. }
                                    if matches!(
                                        name.as_str(),
                                        "CellReadGuard" | "CellEditGuard"
                                    )
                            ))
                ) && matches!(
                    &init.kind,
                    TExprKind::HostCall(host)
                        if matches!(host.as_ref(), THostCall::CellGuardProject { .. })
                ))
        }
        TStmt::ListDestructure { init, elems, .. } => {
            jit_list_native_type(&init.ty)
                && !elems.is_empty()
                && resident_safe_expr(init, callees)
        }
        TStmt::StructDestructure { init, binds, .. } => {
            jit_struct_type(&init.ty) && !binds.is_empty() && resident_safe_expr(init, callees)
        }
        TStmt::Assign {
            place,
            op,
            value,
            clone_value,
            ..
        } => {
            let compound = op.as_ref().is_none_or(|op| match &value.ty {
                Type::Int | Type::IntN { .. } | Type::InlineRange { .. } => matches!(
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
                        // D-EXPSEM1=A / D-FLOORDIV1=A: `^=` and `/%=` call the host.
                        | BinOp::Pow
                        | BinOp::FloorDiv
                        | BinOp::Mod
                ),
                Type::Float | Type::Float32 => {
                    matches!(
                        op,
                        BinOp::Add
                            | BinOp::Sub
                            | BinOp::Mul
                            | BinOp::Div
                            | BinOp::Pow
                            | BinOp::FloorDiv
                    )
                }
                Type::Named(name) if name == "Decimal" => {
                    matches!(op, BinOp::Add | BinOp::Sub | BinOp::Mul)
                }
                Type::String => matches!(op, BinOp::Add),
                _ => false,
            });
            let local = place.as_local().is_some_and(|local| {
                local
                    .name
                    .chars()
                    .all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
            });
            let field = structured_record_field_place(place)
                || matches!(
                    place,
                    TIR::TPlace::Expr(expr)
                        if matches!(
                            &expr.kind,
                            TExprKind::PoolSlot { field: Some(_), .. }
                        )
                );
            let shared_guard_value = matches!(
                place,
                TIR::TPlace::Expr(expr)
                    if matches!(&expr.kind, TExprKind::SharedGuardValue { .. })
            );
            (!clone_value || jit_value_type(&value.ty))
                && compound
                && (local || field || shared_guard_value)
                && resident_safe_expr(value, callees)
        }
        TStmt::Return(ret) => ret.as_ref().is_none_or(|e| resident_safe_expr(e, callees)),
        TStmt::ExprStmt(e) => resident_safe_expr(e, callees),
        // Lowered by JIT (`Close` / file hosts); same expr gate as ExprStmt.
        TStmt::DeferClose { close, .. } => resident_safe_expr(close, callees),
        TStmt::If {
            cond,
            then_body,
            else_body,
            ..
        } => {
            let cond_ok = match cond {
                TIfCond::Plain(e) => matches!(&e.ty, Type::Bool) && resident_safe_expr(e, callees),
                TIfCond::IfLet { pattern, subj } => {
                    matches!(
                        &pattern.pattern,
                        Pattern::Ok { .. }
                            | Pattern::Err { .. }
                            | Pattern::Present { .. }
                            | Pattern::Variant { .. }
                    ) && resident_safe_expr(subj, callees)
                }
                TIfCond::Matches { subj, .. } => resident_safe_expr(subj, callees),
                TIfCond::And { left, right } => {
                    // `if a == .V(x) && …` and similar dual conditions.
                    matches!(
                        (left.as_ref(), right.as_ref()),
                        (
                            TIfCond::Plain(e)
                                | TIfCond::Matches { subj: e, .. }
                                | TIfCond::IfLet { subj: e, .. }
                                | TIfCond::IsNone { subj: e },
                            TIfCond::Plain(e2)
                                | TIfCond::Matches { subj: e2, .. }
                                | TIfCond::IfLet { subj: e2, .. }
                                | TIfCond::IsNone { subj: e2 },
                        ) if resident_safe_expr(e, callees) && resident_safe_expr(e2, callees)
                    ) || {
                        // Recurse for nested And / Plain bool.
                        let left_ok = match left.as_ref() {
                            TIfCond::Plain(e) => {
                                matches!(&e.ty, Type::Bool) && resident_safe_expr(e, callees)
                            }
                            TIfCond::IfLet { subj, .. }
                            | TIfCond::Matches { subj, .. }
                            | TIfCond::IsNone { subj } => resident_safe_expr(subj, callees),
                            TIfCond::And { .. } => false,
                        };
                        let right_ok = match right.as_ref() {
                            TIfCond::Plain(e) => {
                                matches!(&e.ty, Type::Bool) && resident_safe_expr(e, callees)
                            }
                            TIfCond::IfLet { subj, .. }
                            | TIfCond::Matches { subj, .. }
                            | TIfCond::IsNone { subj } => resident_safe_expr(subj, callees),
                            TIfCond::And { .. } => false,
                        };
                        left_ok && right_ok
                    }
                }
                // `if maybe == .None` — Option ABI already lowered in lower_ctx.
                TIfCond::IsNone { subj } => {
                    matches!(&subj.ty, Type::Option(_)) && resident_safe_expr(subj, callees)
                }
            };
            cond_ok
                && then_body.iter().all(|s| resident_safe_stmt(s, callees))
                && else_body
                    .as_ref()
                    .is_none_or(|b| b.iter().all(|s| resident_safe_stmt(s, callees)))
        }
        TStmt::Loop { body, .. } => body.iter().all(|s| resident_safe_stmt(s, callees)),
        TStmt::While { cond, body, .. } => {
            matches!(&cond.ty, Type::Bool)
                && resident_safe_expr(cond, callees)
                && body.iter().all(|s| resident_safe_stmt(s, callees))
        }
        TStmt::CountedLoop {
            init,
            cond,
            step,
            body,
            ..
        } => {
            resident_safe_stmt(init, callees)
                && matches!(&cond.ty, Type::Bool)
                && resident_safe_expr(cond, callees)
                && step.as_ref().is_none_or(|step| resident_safe_stmt(step, callees))
                && body.iter().all(|s| resident_safe_stmt(s, callees))
        }
        // D-RANGE-VALUE1=A: `source` is `Some` for `loop n in <Range value>`.
        // TIR fills `start`/`end` with `IntLit(0)` PLACEHOLDERS in that form
        // (`lower/statements.rs`, the `Type::Named(Range)` collection branch),
        // and the resident lowering reads the real bounds out of `source`
        // through `LowerCtx::lower_range_expr`, ignoring both placeholders. So
        // gating `start`/`end` unconditionally let two literal zeros stand in
        // as proof for a `source` the walker never visited at all — the same
        // shape as reading a per-comparison hook off an operand index
        // (D-CHAINCMP1). Gate whichever pair actually supplies the bounds.
        TStmt::Range {
            source,
            start,
            end,
            step,
            body,
            ..
        } => {
            let bounds = match source {
                Some(source) => {
                    matches!(&source.ty, Type::Named(name) if name == jet_foundation::Syntax::TYPE_RANGE)
                        && resident_safe_expr(source, callees)
                }
                None => {
                    matches!(&start.ty, Type::Int)
                        && matches!(&end.ty, Type::Int)
                        && resident_safe_expr(start, callees)
                        && resident_safe_expr(end, callees)
                }
            };
            bounds
                && step.is_none()
                && body.iter().all(|s| resident_safe_stmt(s, callees))
        }
        TStmt::Break(_) | TStmt::Continue(_) => true,
        TStmt::BreakValue { value, .. } => {
            (jit_value_type(&value.ty)
                || jit_list_native_type(&value.ty)
                || jit_struct_type(&value.ty)
                || jit_tuple_type(&value.ty))
                && resident_safe_expr(value, callees)
        }
        TStmt::IndexAssign {
            base,
            index,
            is_map,
            value,
            ..
        } => {
            if *is_map {
                let key_ok = if jit_map_composite_key_type(&base.ty) {
                    jit_map_composite_key_expr(index)
                } else if jit_map_int_type(&base.ty) {
                    jit_map_int_key_type(&index.ty)
                } else {
                    jit_map_string_type(&base.ty) && matches!(&index.ty, Type::String)
                };
                key_ok
                    && !jit_map_intn_value_type(&base.ty)
                    && jit_value_type(&value.ty)
                    && resident_safe_expr(base, callees)
                    && resident_safe_expr(index, callees)
                    && resident_safe_expr(value, callees)
            } else {
                (jit_list_native_type(&base.ty)
                    || jit_list_iter_elem_type(&base.ty).is_some()
                    || jit_closure_elem_type(&base.ty).is_some()
                    || jit_float_view_mut_type(&base.ty))
                    && matches!(&index.ty, Type::Int)
                    && matches!(
                        &value.ty,
                        Type::Int
                            | Type::IntN { .. }
                            | Type::Float
                            | Type::Float32
                            | Type::String
                            | Type::Char
                            | Type::Bool
                    )
                    && resident_safe_expr(base, callees)
                    && resident_safe_expr(index, callees)
                    && resident_safe_expr(value, callees)
            }
        }
        TStmt::IndexFieldAssign(assign) => {
            let base_ok = jit_list_record_type(&assign.base.ty)
                || matches!(
                    &assign.base.ty,
                    Type::Apply { name, args }
                        if matches!(name.as_str(), "ViewMut" | "ComputeViewMut")
                            && args.len() == 1
                            && (record_type_key(&args[0]).is_some()
                                || matches!(&args[0], Type::TraitObject(_)))
                );
            !assign.is_map
                && !assign.clone_value
                && base_ok
                && matches!(&assign.index.ty, Type::Int)
                && assign
                    .op
                    .is_none_or(|_| matches!(&assign.field_ty, Type::Int | Type::Float))
                && resident_safe_expr(&assign.base, callees)
                && resident_safe_expr(&assign.index, callees)
                && resident_safe_expr(&assign.value, callees)
        }
        TStmt::ForIn {
            var2,
            source,
            collection,
            method_kind,
            columnar,
            by_value,
            step,
            body,
            ..
        } => {
            use jet_codegen::Codegen::TIR::TForInMethod;
            let chars_ok = matches!(method_kind, Some(TForInMethod::Chars))
                && matches!(&source.ty, Type::String);
            let channel_ok = matches!(method_kind, Some(TForInMethod::ChannelReceiver))
                && var2.is_none()
                && matches!(&collection.ty, Type::Apply { name, args } if name == "Receiver" && args.len() == 1)
                && jit_concurrency_type(&collection.ty);
            let iterable_ok = matches!(
                method_kind,
                Some(TForInMethod::Iterable { coll_type, iter_type })
                    if record_type_key(&source.ty).is_some_and(|name| name == *coll_type)
                        && callees.contains(&format!("{coll_type}::iter"))
                        && callees.contains(&format!("{iter_type}::next"))
            );
            let process_lines_ok = matches!(method_kind, Some(TForInMethod::LinesProcessStream))
                && var2.is_none()
                && matches!(
                    &source.kind,
                    TExprKind::Field { recv, field, boxed: false }
                        if matches!(&recv.ty, Type::Named(n) if n == "ProcessChild")
                            && (field == "stdout" || field == "stderr")
                );
            let file_lines_ok = matches!(method_kind, Some(TForInMethod::LinesFile)) && var2.is_none();
            let stdin_lines_ok =
                matches!(method_kind, Some(TForInMethod::LinesStdin)) && var2.is_none();
            let encoding_reader_ok = matches!(
                method_kind,
                Some(TForInMethod::EncodingReader { reader_type })
                    if var2.is_none()
                        && matches!(&collection.ty, Type::Named(name) if name == reader_type)
                        && matches!(
                            reader_type.as_str(),
                            "JSONReader" | "JSONLReader" | "CSVReader" | "XMLReader" | "CBORReader"
                        )
            );
            // TIR leaves `.lines()` MethodCall as Unit and the loop var unbound;
            // lower hardcodes String elems. Don't demand collection/body types.
            //
            // D-SOA-TIER1=A: `columnar` is consulted at exactly ONE place in this
            // predicate — the encoding-reader shape below — because that is the
            // only place the lowering still consults it. Every other for-in shape
            // walks the logical rows, which is what a columnar list IS on this
            // tier, so refusing it here would refuse a loop the lowering compiles.
            // Predicate and lowering admit the same set; a wider predicate would
            // be a Cranelift rejection of generated code, i.e. an ICE (I2), and a
            // narrower one would leave the host dead.
            if channel_ok {
                return resident_safe_expr(source, callees)
                    && step.as_ref().is_none_or(|step| resident_safe_expr(step, callees))
                    && body.iter().all(|s| resident_safe_stmt(s, callees));
            }
            if process_lines_ok || file_lines_ok || stdin_lines_ok {
                return resident_safe_expr(source, callees)
                    && step
                        .as_ref()
                        .is_none_or(|step| resident_safe_expr(step, callees))
                    && body.iter().all(|s| resident_safe_process_lines_body(s, callees));
            }
            // Encoding readers pull through the same bounded Prelude codec host
            // used by `HandleMethod::JSON*ReaderNext` and friends. The resident
            // lowering keeps that pull/error/EOF contract while materializing each
            // yielded item into the loop binding; it never re-parses the stream.
            if encoding_reader_ok {
                return !columnar
                    && resident_safe_expr(source, callees)
                    && resident_safe_expr(collection, callees)
                    && step
                        .as_ref()
                        .is_none_or(|step| resident_safe_expr(step, callees))
                    && body.iter().all(|s| resident_safe_stmt(s, callees));
            }
            let list_ok = method_kind.is_none()
                && var2.is_none()
                && (jit_list_iter_elem_type(&collection.ty).is_some()
                    || jit_closure_elem_type(&collection.ty).is_some()
                    || jit_list_record_type(&collection.ty));
            let stream_ok = method_kind.is_none()
                && var2.is_none()
                && step.is_none()
                && matches!(
                    &collection.ty,
                    Type::Apply { name, args }
                        if name == "Stream"
                            && args.len() == 1
                            && jit_value_type(&args[0])
                );
            // D-RANGE-EXCL1=C: sequence two-binding is index then item.
            let list_pair_ok = method_kind.is_none()
                && var2.is_some()
                && (jit_list_iter_elem_type(&collection.ty).is_some()
                    || jit_closure_elem_type(&collection.ty).is_some()
                    || jit_list_record_type(&collection.ty));
            let map_ok = method_kind.is_none()
                && var2.is_some()
                && jit_map_string_type(&collection.ty);
            // `by_value` marks Stream/Iter/HTTPBodyChunks/moved lists. List and
            // FixedList materialize as handles; true lazy Stream stays out.
            let by_value_ok = !*by_value
                || jet_foundation::Collections::is_iter_type(&collection.ty)
                || matches!(
                    &collection.ty,
                    Type::List(_) | Type::FixedList { .. }
                )
                || matches!(&collection.ty, Type::Apply { name, .. } if name == "Stream");
            (chars_ok || iterable_ok || stream_ok || list_ok || list_pair_ok || map_ok)
                && by_value_ok
                && resident_safe_expr(source, callees)
                && resident_safe_expr(collection, callees)
                && step.as_ref().is_none_or(|step| resident_safe_expr(step, callees))
                && body.iter().all(|s| resident_safe_stmt(s, callees))
        }
        TStmt::EnumMatch {
            scrutinee,
            arms,
            else_body,
            ..
        } => {
            let scrutinee_ok = resident_safe_expr(scrutinee, callees)
                && (is_packed_process_signal(scrutinee)
                    || matches!(&scrutinee.ty, Type::Option(_) | Type::Result { .. })
                    || jit_enum_type(&scrutinee.ty));
            scrutinee_ok
                && arms
                    .iter()
                    .all(|a| a.body.iter().all(|s| resident_safe_stmt(s, callees)))
                && else_body
                    .as_ref()
                    .is_none_or(|b| b.iter().all(|s| resident_safe_stmt(s, callees)))
        }
        TStmt::MixedSwitch {
            subject,
            arms,
            else_body,
            ..
        } => {
            resident_safe_expr(subject, callees)
                && arms.iter()
                .all(|(_, b)| b.iter().all(|s| resident_safe_stmt(s, callees)))
                && else_body
                    .as_ref()
                    .is_none_or(|b| b.iter().all(|s| resident_safe_stmt(s, callees)))
        }
        TStmt::RangeSwitch {
            subject,
            arms,
            else_body,
        } => {
            resident_safe_expr(subject, callees)
                && arms
                    .iter()
                    .all(|(_, _, body)| body.iter().all(|s| resident_safe_stmt(s, callees)))
                && else_body.iter().all(|s| resident_safe_stmt(s, callees))
        }
        TStmt::TaskGroup { body, .. }
        | TStmt::Region(body)
        | TStmt::Impure(body)
        | TStmt::Inline(body)
        | TStmt::Unsafe { body, .. }
        | TStmt::SentryPolicy { body, .. }
        | TStmt::Shield { body }
        | TStmt::Transact { body, .. }
        | TStmt::Live { body } => {
            body.iter().all(|s| resident_safe_stmt(s, callees))
        }
        TStmt::ContextBlock { guards, body } => {
            guards
                .iter()
                .all(|(_, value)| resident_safe_expr(value, callees))
                && body.iter().all(|s| resident_safe_stmt(s, callees))
        }
        TStmt::Reactive { executable, .. } => match &executable.executable {
            TIR::TLambdaBody::Expr(e) => resident_safe_expr(e, callees),
            TIR::TLambdaBody::Block(stmts) => stmts.iter().all(|s| resident_safe_stmt(s, callees)),
            TIR::TLambdaBody::SharedBlock(stmts) => {
                stmts.iter().all(|s| resident_safe_stmt(s, callees))
            }
        },
        TStmt::Layout { body, .. } => body.iter().all(|s| resident_safe_stmt(s, callees)),
        TStmt::IndexHookAssign {
            base,
            index,
            value,
            ..
        } => {
            resident_safe_expr(base, callees)
                && resident_safe_expr(index, callees)
                && resident_safe_expr(value, callees)
        }
        // JIT models singleton split views as checked element/window handles.
        // Range windows need relative end-bound enforcement before they can be
        // resident-safe.
        TStmt::SplitViews {
            owner,
            single,
            elem_ty,
            ..
        } => {
            *single
                && elem_ty.as_ref().is_some_and(|ty| {
                    matches!(ty, Type::Int | Type::Float | Type::String)
                        || record_type_key(ty).is_some()
                })
                && owner
                    .as_ref()
                    .is_none_or(|o| resident_safe_expr(o, callees))
        }
        // Whole-value GC assign (`replace_all`): nested stmt computes the new
        // payload, then the collector-backed host edits the root.
        TStmt::GcEdit {
            replace_all,
            index_temp,
            stmt,
            ..
        } => {
            let resident_string_field_compound = matches!(
                stmt.as_ref(),
                TStmt::Assign {
                    place,
                    op: Some(BinOp::Add),
                    value,
                    ..
                } if structured_record_field_place(place)
                    && matches!(&value.ty, Type::String)
            );
            (*replace_all || resident_string_field_compound)
                && index_temp
                    .as_ref()
                    .is_none_or(|(_, e)| resident_safe_expr(e, callees))
                && resident_safe_stmt(stmt, callees)
        }
        TStmt::MathSwizzleAssign { base, value, .. } => {
            resident_safe_expr(base, callees) && resident_safe_expr(value, callees)
        }
        // Pattern capture and the diverging miss route stay in the canonical
        // TIR evaluator until Cranelift has a native pattern-binding lowering.
        // Returning false here makes tier planning deopt the containing
        // function instead of duplicating that policy in the resident engine.
        TStmt::RefutableBind { .. } => false,
        _ => false,
    }
}

fn structured_record_field_place(place: &TIR::TPlace) -> bool {
    let TIR::TPlace::Expr(expr) = place else {
        return false;
    };
    matches!(
        &expr.kind,
        TIR::TExprKind::Field {
            recv,
            boxed: false,
            ..
        } if matches!(
            &recv.kind,
            TIR::TExprKind::Local(_) | TIR::TExprKind::PoolSlot { .. }
        )
    )
}

pub(crate) fn resident_safe_func(tir: &TFunc, callees: &HashSet<String>) -> bool {
    resident_safe_func_detail(tir, callees).is_none()
}

pub(crate) fn resident_safe_func_detail(tir: &TFunc, callees: &HashSet<String>) -> Option<String> {
    if !matches!(
        tir.kind,
        TFuncKind::TopLevel | TFuncKind::Method { .. } | TFuncKind::TraitMethod { .. }
    ) {
        return Some("not top-level".into());
    }
    if !tir.generics.is_empty() || tir.is_unsafe || tir.is_reactive {
        return Some("func attrs unsupported".into());
    }
    if let Some(proof) = tir.kernel_proof {
        if !proof.is_complete() {
            return Some("kernel proof incomplete".into());
        }
    }
    if !tir.params.iter().all(|(_, ty, _)| jit_value_type(ty)) {
        return Some("param type unsupported".into());
    }
    if let Some(ret) = &tir.ret {
        if !jit_value_type(ret) {
            return Some("return type unsupported".into());
        }
    }
    for (i, s) in tir.body.iter().enumerate() {
        if !resident_safe_stmt(s, callees) {
            // One detail ladder (I8). `first_unsafe_stmt_detail` is total over
            // `TStmt` — every arm returns `Some` for a statement the predicate
            // already refused — so this caller only prefixes the body index. A
            // second copy of the ladder here was unreachable and could drift.
            let drill = first_unsafe_stmt_detail(std::slice::from_ref(s), callees)
                .unwrap_or_else(|| stmt_kind_tag(s).to_string());
            return Some(format!("body stmt {i}: {drill}"));
        }
    }
    None
}

/// Name the construct behind a resident-safety refusal.
///
/// Every `TExprKind` variant is named explicitly and there is deliberately no
/// `_` fallback: the old catch-all reported `init=OtherExpr`, which HID the
/// identity of real refusals and made them unactionable. A variant added to
/// `TExprKind` must fail to compile here rather than silently become
/// unnameable — the same convention the `TBuiltinOp` dispatch uses. `TExprKind`
/// has no derives, so enumeration is the mechanism (there is no `Debug`).
///
/// Arms follow the declaration order of `TExprKind` in
/// `crates/jet-codegen/src/Codegen/TIR/mod.rs` so coverage is checkable by eye.
fn expr_kind_name(kind: &TExprKind) -> &'static str {
    match kind {
        TExprKind::IntLit(_, _) => "IntLit",
        TExprKind::FloatLit(_) => "FloatLit",
        TExprKind::BoolLit(_) => "BoolLit",
        TExprKind::CharLit(_) => "CharLit",
        TExprKind::StrLit(_) => "StrLit",
        TExprKind::Local(_) => "Local",
        TExprKind::Unit => "Unit",
        TExprKind::InlineBlock(_) => "InlineBlock",
        TExprKind::DefaultLit => "DefaultLit",
        TExprKind::Uninit => "Uninit",
        TExprKind::CtLit(_) => "CtLit",
        TExprKind::HostCall(_) => "HostCall",
        TExprKind::ConstRef(_) => "ConstRef",
        TExprKind::DataEntriesToMap(_) => "DataEntriesToMap",
        TExprKind::Call { .. } => "Call",
        TExprKind::DistinctCtor { .. } => "DistinctCtor",
        TExprKind::RangeCheckedCtor { .. } => "RangeCheckedCtor",
        TExprKind::DistinctConvert { .. } => "DistinctConvert",
        TExprKind::UnitConvert { .. } => "UnitConvert",
        TExprKind::MathBuiltin { .. } => "MathBuiltin",
        TExprKind::PreciseBuiltin { .. } => "PreciseBuiltin",
        TExprKind::Print(_) => "Print",
        TExprKind::Drop(_) => "Drop",
        TExprKind::Close(_) => "Close",
        TExprKind::ResourceNew(_) => "ResourceNew",
        TExprKind::ResourceTake(_) => "ResourceTake",
        TExprKind::AmbientInput { .. } => "AmbientInput",
        TExprKind::RequireStop { .. } => "RequireStop",
        TExprKind::Binary { .. } => "Binary",
        TExprKind::CompareChain { .. } => "CompareChain",
        TExprKind::LayoutCompare { .. } => "LayoutCompare",
        TExprKind::LayoutLit { .. } => "LayoutLit",
        TExprKind::Unary { .. } => "Unary",
        TExprKind::IncDec { .. } => "IncDec",
        TExprKind::StructLit { .. } => "StructLit",
        TExprKind::Field { .. } => "Field",
        TExprKind::SharedGuardValue { .. } => "SharedGuardValue",
        TExprKind::SharedGuardMap { .. } => "SharedGuardMap",
        TExprKind::SharedGuardSplit { .. } => "SharedGuardSplit",
        TExprKind::SharedGuardWait { .. } => "SharedGuardWait",
        TExprKind::ConditionNotify { .. } => "ConditionNotify",
        TExprKind::PtrFromAddr { .. } => "PtrFromAddr",
        TExprKind::Deref(_) => "Deref",
        TExprKind::RawOf(_) => "RawOf",
        TExprKind::AllocNew { .. } => "AllocNew",
        TExprKind::EnumLit { .. } => "EnumLit",
        TExprKind::JSONLit { .. } => "JSONLit",
        TExprKind::DBValueLit { .. } => "DBValueLit",
        TExprKind::ListLit(_) => "ListLit",
        TExprKind::ListSpread { .. } => "ListSpread",
        TExprKind::ColumnarListLit { .. } => "ColumnarListLit",
        TExprKind::ColumnarGather { .. } => "ColumnarGather",
        TExprKind::ColumnarColumnRead { .. } => "ColumnarColumnRead",
        TExprKind::TupleLit { .. } => "TupleLit",
        TExprKind::MapLit(_) => "MapLit",
        TExprKind::Index { .. } => "Index",
        TExprKind::PoolSlot { .. } => "PoolSlot",
        TExprKind::IndexHook { .. } => "IndexHook",
        TExprKind::MathLaneIndex { .. } => "MathLaneIndex",
        TExprKind::MathSwizzleRead { .. } => "MathSwizzleRead",
        TExprKind::Slice { .. } => "Slice",
        TExprKind::Clone(_) => "Clone",
        TExprKind::ExplicitCopy(_) => "ExplicitCopy",
        TExprKind::Borrow { .. } => "Borrow",
        TExprKind::MaterializeView(_) => "MaterializeView",
        TExprKind::MethodCall { .. } => "MethodCall",
        TExprKind::FnFieldCall { .. } => "FnFieldCall",
        TExprKind::StaticCall { .. } => "StaticCall",
        TExprKind::DecodeUnder { .. } => "DecodeUnder",
        TExprKind::BuiltinMethod { .. } => "BuiltinMethod",
        TExprKind::CoreCall { .. } => "CoreCall",
        TExprKind::IfExpr { .. } => "IfExpr",
        TExprKind::Todo { .. } => "Todo",
        TExprKind::Unreachable { .. } => "Unreachable",
        TExprKind::DistinctRaw(_) => "DistinctRaw",
        TExprKind::Present(_) => "Present",
        TExprKind::Absent => "Absent",
        TExprKind::Ok(_) => "Ok",
        TExprKind::Err(_) => "Err",
        TExprKind::Try { .. } => "Try",
        TExprKind::OrFallback { .. } => "OrFallback",
        TExprKind::OptField { .. } => "OptField",
        TExprKind::Lambda(_) => "Lambda",
        TExprKind::PatternMatches { .. } => "PatternMatches",
        TExprKind::OptionLift2 { .. } => "OptionLift2",
        TExprKind::ClosureMethod { .. } => "ClosureMethod",
        TExprKind::HostBorrowCallback { .. } => "HostBorrowCallback",
        TExprKind::NumericMethod { .. } => "NumericMethod",
        TExprKind::OverflowOpt { .. } => "OverflowOpt",
        TExprKind::HandleMethod { .. } => "HandleMethod",
        // The closure kinds are named for the same reason: the callback form is
        // the fact a refusal turns on.
        TExprKind::CoreClosureCall { kind } => match kind {
            TCoreClosureKind::Spawn { .. } => "CoreClosure:Spawn",
            TCoreClosureKind::Serve { .. } => "CoreClosure:Serve",
            TCoreClosureKind::OnInterrupt { .. } => "CoreClosure:OnInterrupt",
            TCoreClosureKind::Guard { .. } => "CoreClosure:Guard",
            TCoreClosureKind::OnCommit { .. } => "CoreClosure:OnCommit",
            TCoreClosureKind::OnRollback { .. } => "CoreClosure:OnRollback",
            TCoreClosureKind::ReactiveDerived { .. } => "CoreClosure:Derived",
            TCoreClosureKind::ReactiveEffect { .. } => "CoreClosure:Effect",
            TCoreClosureKind::UiReactiveRender { .. } => "CoreClosure:UiRender",
            TCoreClosureKind::UiButtonOnClick { .. } => "CoreClosure:UiButtonOnClick",
        },
        TExprKind::TaskGroupAll { .. } => "TaskGroupAll",
        TExprKind::TaskGroupRace { .. } => "TaskGroupRace",
        TExprKind::TaskGroupAny { .. } => "TaskGroupAny",
        TExprKind::SelectStart => "SelectStart",
        TExprKind::SelectRecv { .. } => "SelectRecv",
        TExprKind::SelectAfter { .. } => "SelectAfter",
        TExprKind::SelectWait { .. } => "SelectWait",
        TExprKind::FnValue { .. } => "FnValue",
        TExprKind::ModuleCall { .. } => "ModuleCall",
        TExprKind::ExternCall { .. } => "ExternCall",
    }
}

/// The refusal tag: the variant name, plus the one fact the refusal actually
/// turns on when the variant alone is not the answer — the resolved callee for
/// the two call forms, and the op for the two method dispatches. `TBuiltinOp`
/// and `TClosureOp` derive `Debug`, so naming them needs no second table.
fn expr_kind_tag(expr: &TExpr) -> String {
    match &expr.kind {
        TExprKind::Call { name, .. } => format!("Call:{name}"),
        TExprKind::CoreCall { module, method, .. } => format!("CoreCall:{module}.{method}"),
        TExprKind::BuiltinMethod { op, .. } => format!("BuiltinMethod:{op:?}"),
        TExprKind::ClosureMethod { op, .. } => format!("ClosureMethod:{op:?}"),
        kind => expr_kind_name(kind).to_string(),
    }
}

fn stmt_kind_tag(stmt: &TStmt) -> &'static str {
    match stmt {
        TStmt::Contract { .. } => "Contract",
        TStmt::ContractScope { .. } => "ContractScope",
        TStmt::Let { .. } => "Let",
        TStmt::RefutableBind { .. } => "RefutableBind",
        TStmt::Assign { .. } => "Assign",
        TStmt::IndexAssign { .. } => "IndexAssign",
        TStmt::IndexFieldAssign(_) => "IndexFieldAssign",
        TStmt::SplitViews { .. } => "SplitViews",
        TStmt::LineMarker(_) => "LineMarker",
        TStmt::Return(_) => "Return",
        TStmt::ExprStmt(_) => "ExprStmt",
        TStmt::Reactive { .. } => "Reactive",
        TStmt::If { .. } => "If",
        TStmt::Loop { .. } => "Loop",
        TStmt::While { .. } => "While",
        TStmt::ForIn { .. } => "ForIn",
        TStmt::Inline(_) => "Inline",
        TStmt::Impure(_) => "Impure",
        TStmt::Region(_) => "Region",
        TStmt::SentryPolicy { .. } => "SentryPolicy",
        TStmt::TaskGroup { .. } => "TaskGroup",
        TStmt::Unsafe { .. } => "Unsafe",
        TStmt::SourceSpan(_) => "SourceSpan",
        _ => "Other",
    }
}

/// Name a pattern shape for the refusal ladder. Total over `Pattern`, and with no
/// `_` fallback, for the same reason `expr_kind_name` is: an `if let` refused for
/// its pattern must say which pattern.
fn pattern_kind_tag(pattern: &Pattern) -> &'static str {
    match pattern {
        Pattern::Variant { .. } => "Variant",
        Pattern::Present { .. } => "Present",
        Pattern::Absent(_) => "Absent",
        Pattern::Ok { .. } => "Ok",
        Pattern::Err { .. } => "Err",
        Pattern::Range { .. } => "Range",
        Pattern::Or(_, _) => "Or",
        Pattern::Struct { .. } => "Struct",
        Pattern::StrMatch { .. } => "StrMatch",
        Pattern::BinMatch { .. } => "BinMatch",
    }
}

/// Walk the single-operand wrappers whose safety answer IS their operand's, so a
/// refused `print(x)` names `x` instead of stopping at `Print`.
///
/// Every variant here has one arm in `resident_safe_expr_work_item` of the shape
/// `Some((true, vec![operand]))`, which means the wrapper adds no gate of its own
/// and the operand carries the whole refusal. `Print` in particular reports its
/// own `Unit` type, which named nothing at all. `??` is here too: it carries two
/// candidate expressions, and `or_fallback_children` already says which.
fn refusal_operand_chain(expr: &TExpr, callees: &HashSet<String>) -> String {
    let mut out = String::new();
    let mut cur = expr;
    loop {
        let operand: &TExpr = match &cur.kind {
            TExprKind::Print(inner)
            | TExprKind::Present(inner)
            | TExprKind::Ok(inner)
            | TExprKind::Err(inner)
            | TExprKind::Drop(inner)
            | TExprKind::MaterializeView(inner)
            | TExprKind::ResourceNew(inner)
            | TExprKind::DistinctConvert { arg: inner, .. }
            | TExprKind::DistinctRaw(inner)
            | TExprKind::Clone(inner) => inner.as_ref(),
            // `??` carries two candidate expressions and `or_fallback_children`
            // is the one table that says which; name the first it refuses.
            TExprKind::OrFallback { value, fallback } => {
                match or_fallback_children(value, fallback)
                    .into_iter()
                    .find(|child| !resident_safe_expr(child, callees))
                {
                    Some(child) => child,
                    None => return out,
                }
            }
            _ => return out,
        };
        out.push_str(&format!(
            " > {} ty={:?}",
            expr_kind_tag(operand),
            operand.ty
        ));
        cur = operand;
    }
}

/// Which half of a refused `if` refused: a branch statement, or the condition.
///
/// `resident_safe_stmt`'s `If` arm is `cond_ok && every branch statement`, so a
/// clean pair of branches leaves the condition as the only candidate. The
/// condition is then named by shape — this states no rule of its own.
fn if_cond_tag(cond: &TIfCond) -> String {
    match cond {
        TIfCond::Plain(e) => format!("Plain:{} ty={:?}", expr_kind_tag(e), e.ty),
        TIfCond::IfLet { pattern, subj } => format!(
            "IfLet:{} subj={} ty={:?}",
            pattern_kind_tag(&pattern.pattern),
            expr_kind_tag(subj),
            subj.ty
        ),
        TIfCond::Matches { subj, .. } => {
            format!("Matches subj={} ty={:?}", expr_kind_tag(subj), subj.ty)
        }
        TIfCond::IsNone { subj } => {
            format!("IsNone subj={} ty={:?}", expr_kind_tag(subj), subj.ty)
        }
        TIfCond::And { left, right } => {
            format!("And({} , {})", if_cond_tag(left), if_cond_tag(right))
        }
    }
}

fn first_unsafe_stmt_detail(stmts: &[TStmt], callees: &HashSet<String>) -> Option<String> {
    for (i, s) in stmts.iter().enumerate() {
        if resident_safe_stmt(s, callees) {
            continue;
        }
        match s {
            TStmt::Inline(body)
            | TStmt::Impure(body)
            | TStmt::Region(body)
            | TStmt::TaskGroup { body, .. }
            | TStmt::Unsafe { body, .. }
            | TStmt::SentryPolicy { body, .. } => {
                if let Some(inner) = first_unsafe_stmt_detail(body, callees) {
                    return Some(format!("{}[{i}]>{inner}", stmt_kind_tag(s)));
                }
                return Some(format!("{}[{i}]", stmt_kind_tag(s)));
            }
            // `TStmt::Return`'s gate IS its value's (`ret.as_ref().is_none_or(…)`
            // above), so a refused `return` carries an expression and belongs in
            // the same drill as the other value-carrying statements. It used to
            // fall to the `_` arm and report a bare `Return[i]`, which named no
            // construct at all — the fallible-void `Ok(Unit)` refusal cost a
            // session to identify for exactly that reason. `Return(None)` never
            // reaches here: the predicate always admits it.
            TStmt::Let { init, .. }
            | TStmt::ExprStmt(init)
            | TStmt::Assign { value: init, .. }
            | TStmt::Return(Some(init)) => {
                let mut detail = format!("{}[{i}]", stmt_kind_tag(s));
                if let TStmt::Let { name, .. } = s {
                    // The Jet binding name pins the refusal to ONE source
                    // statement. A variant name alone leaves every `Let` in the
                    // body as a candidate, which is what made the last two
                    // collection refusals cost a session each.
                    detail.push_str(&format!(" `{name}`"));
                }
                detail.push_str(&format!(" init={} ty={:?}", expr_kind_tag(init), init.ty));
                detail.push_str(&refusal_operand_chain(init, callees));
                if let TExprKind::CoreClosureCall {
                    kind: TCoreClosureKind::ReactiveDerived { executable, .. }
                        | TCoreClosureKind::ReactiveEffect { executable, .. }
                        | TCoreClosureKind::UiReactiveRender { executable, .. }
                        | TCoreClosureKind::UiButtonOnClick { executable, .. },
                } = &init.kind
                {
                    match &executable.executable {
                        TIR::TLambdaBody::Expr(e) => {
                            detail.push_str(&format!(" body={}", expr_kind_tag(e)));
                            if let TExprKind::Binary {
                                overflow,
                                lhs,
                                rhs,
                                ..
                            } = &e.kind
                            {
                                let recv_ty = if let TExprKind::HandleMethod { recv, op, .. } =
                                    &lhs.kind
                                {
                                    format!("recv={:?} op_is_get={}", recv.ty, matches!(op, THandleOp::ReactiveGet))
                                } else {
                                    "recv=?".into()
                                };
                                detail.push_str(&format!(
                                    " bin=({}/{} overflow={overflow} lhs_ok={} rhs_ok={} lhs_ty={:?} rhs_ty={:?} {recv_ty})",
                                    expr_kind_tag(lhs),
                                    expr_kind_tag(rhs),
                                    intish_ty(&lhs.ty) || reactive_get_intish(lhs),
                                    intish_ty(&rhs.ty) || reactive_get_intish(rhs),
                                    lhs.ty,
                                    rhs.ty,
                                ));
                            }
                        }
                        TIR::TLambdaBody::Block(inner) => {
                            if let Some(b) = first_unsafe_stmt_detail(inner, callees) {
                                detail.push_str(&format!(" block>{b}"));
                            }
                        }
                        TIR::TLambdaBody::SharedBlock(inner) => {
                            if let Some(b) = first_unsafe_stmt_detail(&inner[..], callees) {
                                detail.push_str(&format!(" block>{b}"));
                            }
                        }
                    }
                }
                if let TExprKind::HandleMethod { op, .. } = &init.kind {
                    let op_tag = match op {
                        THandleOp::UiBackendMethod { method } => format!("UiBackend:{method}"),
                        THandleOp::EventMethod { method } => format!("Event:{method}"),
                        THandleOp::ReactiveGet => "ReactiveGet".to_string(),
                        THandleOp::ReactiveSet => "ReactiveSet".to_string(),
                        _ => "HandleOp".to_string(),
                    };
                    detail.push_str(&format!(" op={op_tag}"));
                }
                return Some(detail);
            }
            TStmt::If {
                cond,
                then_body,
                else_body,
                ..
            } => {
                // `If[i]` alone left both branches and every condition shape as
                // candidates. `resident_safe_stmt` refuses an `if` for exactly one
                // of three reasons, so ask them in that order and name the one
                // that answered.
                let mut detail = format!("{}[{i}]", stmt_kind_tag(s));
                if let Some(inner) = first_unsafe_stmt_detail(then_body, callees) {
                    detail.push_str(&format!(" then>{inner}"));
                } else if let Some(inner) = else_body
                    .as_ref()
                    .and_then(|body| first_unsafe_stmt_detail(body, callees))
                {
                    detail.push_str(&format!(" else>{inner}"));
                } else {
                    detail.push_str(&format!(" cond={}", if_cond_tag(cond)));
                }
                return Some(detail);
            }
            _ => return Some(format!("{}[{i}]", stmt_kind_tag(s))),
        }
    }
    None
}

pub(crate) fn resident_safe_program(program: &JitProgram) -> bool {
    let names: HashSet<String> = program.funcs.iter().map(|f| f.name.clone()).collect();
    let main_ok = if program.entry == jet_foundation::Names::mangle_generated("cli_main") {
        program
            .funcs
            .iter()
            .any(|f| f.name == "run" && resident_safe_func(f, &names))
    } else {
        program.funcs.iter().any(|f| {
            f.name == program.entry
                && f.params.is_empty()
                && entry_return_supported(f.ret.as_ref())
                && resident_safe_func(f, &names)
        })
    };
    if !main_ok {
        return false;
    }
    if !program.funcs.iter().all(|f| resident_safe_func(f, &names)) {
        return false;
    }
    let spawn_sites = count_spawn_sites(program);
    if spawn_sites != program.spawn_lambdas.len() {
        return false;
    }
    program
        .spawn_lambdas
        .iter()
        .all(|lam| resident_safe_spawn_lambda(lam, &names))
}

/// The spawn-lambda table entries a program references.
///
/// Two kinds of reference, and they do not count the same way.
///
/// A site TIR already assigned (`Spawn`, the reactive closure kinds,
/// `ui.button`, `watch`) carries its table index on the node, and the lowering
/// READS that number (`LowerCtx::lower_spawn`), so N references to one index are
/// still ONE entry. The fenced-name fan `@[ t1..t8 ]@ :: task f()` is exactly
/// that shape: the token expansion copies one statement, every copy keeps the
/// source span, and `jit_spawn_site_with` keys the table by `(fn, span)` — eight
/// spawn expressions, one lambda, all eight running the same body.
///
/// A callback whose index still comes from the traversal cursor (`#Reactive`,
/// `game on_frame`, `event on`/`once`/`on_priority`, the gtk `on_click`) consumes
/// a fresh slot per occurrence, in this walk's order.
#[derive(Default)]
struct SpawnSiteTally {
    assigned: HashSet<usize>,
    cursor: usize,
}

impl SpawnSiteTally {
    fn assigned_site(&mut self, site: usize) {
        self.assigned.insert(site);
    }

    fn cursor_slot(&mut self) {
        self.cursor += 1;
    }

    fn total(&self) -> usize {
        self.assigned.len() + self.cursor
    }
}

/// How many spawn-lambda table entries this program references.
///
/// The planner compares this against `spawn_lambdas.len()` to ask "is every
/// entry referenced, and does every reference resolve". Comparing a raw
/// EXPRESSION count against a TABLE SIZE answered no for any program that
/// referenced one entry twice: `examples/features/memory/shared_transact.jet`
/// was refused with `spawn site count 8 != lambda count 1` for a fan whose
/// eight tasks correctly share one lambda.
pub(crate) fn count_spawn_sites(program: &JitProgram) -> usize {
    let mut n = SpawnSiteTally::default();
    for f in &program.funcs {
        count_spawn_sites_stmts(&f.body, &mut n);
    }
    // A task lambda can contain another task/combinator. Its spawn expression
    // lives in the lambda table rather than in the enclosing function body,
    // so count those nested sites as well. Each table entry is visited once;
    // the enclosing expression still accounts for the entry that launched it.
    for lambda in &program.spawn_lambdas {
        match &lambda.body {
            TJitSpawnBody::Expr(expr) => count_spawn_sites_expr(expr, &mut n),
            TJitSpawnBody::Block { prefix, tail } => {
                count_spawn_sites_stmts(prefix, &mut n);
                if let Some(tail) = tail {
                    count_spawn_sites_expr(tail, &mut n);
                }
            }
            TJitSpawnBody::SharedBlock { body, .. } => {
                count_spawn_sites_stmts(body, &mut n);
            }
        }
    }
    n.total()
}

fn count_spawn_sites_stmts(stmts: &[TStmt], n: &mut SpawnSiteTally) {
    for s in stmts {
        match s {
            TStmt::Contract { contract } => {
                count_spawn_sites_expr(&contract.condition, n);
                count_spawn_sites_expr(&contract.message, n);
            }
            TStmt::ContractScope {
                pre,
                body,
                post,
                ..
            } => {
                for contract in pre.iter().chain(post) {
                    count_spawn_sites_expr(&contract.condition, n);
                    count_spawn_sites_expr(&contract.message, n);
                }
                count_spawn_sites_stmts(body, n);
            }
            TStmt::Let { init, .. }
            | TStmt::Assign { value: init, .. }
            | TStmt::Return(Some(init))
            | TStmt::ExprStmt(init) => count_spawn_sites_expr(init, n),
            TStmt::IndexFieldAssign(assign) => {
                count_spawn_sites_expr(&assign.base, n);
                count_spawn_sites_expr(&assign.index, n);
                count_spawn_sites_expr(&assign.value, n);
            }
            TStmt::If {
                then_body,
                else_body,
                ..
            } => {
                count_spawn_sites_stmts(then_body, n);
                if let Some(b) = else_body {
                    count_spawn_sites_stmts(b, n);
                }
            }
            TStmt::Loop { body, .. } | TStmt::While { body, .. } | TStmt::Range { body, .. } => {
                count_spawn_sites_stmts(body, n)
            }
            TStmt::CountedLoop {
                init, step, body, ..
            } => {
                count_spawn_sites_stmts(std::slice::from_ref(init), n);
                if let Some(step) = step {
                    count_spawn_sites_stmts(std::slice::from_ref(step), n);
                }
                count_spawn_sites_stmts(body, n);
            }
            TStmt::TaskGroup { body, .. }
            | TStmt::Region(body)
            | TStmt::Impure(body)
            | TStmt::Inline(body)
            | TStmt::Unsafe { body, .. }
            | TStmt::SentryPolicy { body, .. }
            | TStmt::Shield { body }
            | TStmt::DebugOnly(body) => count_spawn_sites_stmts(body, n),
            // `#Reactive` has no site on its TIR node, so its table index comes
            // from the traversal cursor (`LowerCtx`'s `TStmt::Reactive` arm).
            TStmt::Reactive { .. } => n.cursor_slot(),
            TStmt::Layout { body, .. } => count_spawn_sites_stmts(body, n),
            TStmt::ContextBlock { guards, body } => {
                for (_, value) in guards {
                    count_spawn_sites_expr(value, n);
                }
                count_spawn_sites_stmts(body, n);
            }
            TStmt::Transact { body, .. } => count_spawn_sites_stmts(body, n),
            TStmt::GcEdit { index_temp, stmt, .. } => {
                if let Some((_, idx)) = index_temp {
                    count_spawn_sites_expr(idx, n);
                }
                count_spawn_sites_stmts(std::slice::from_ref(stmt), n);
            }
            TStmt::ForIn {
                source,
                collection,
                step,
                body,
                ..
            } => {
                count_spawn_sites_expr(source, n);
                count_spawn_sites_expr(collection, n);
                if let Some(step) = step {
                    count_spawn_sites_expr(step, n);
                }
                count_spawn_sites_stmts(body, n);
            }
            TStmt::TupleDestructure { init, .. }
            | TStmt::StructDestructure { init, .. }
            | TStmt::ListDestructure { init, .. } => count_spawn_sites_expr(init, n),
            TStmt::EnumMatch {
                scrutinee,
                arms,
                else_body,
                ..
            } => {
                count_spawn_sites_expr(scrutinee, n);
                for arm in arms {
                    count_spawn_sites_stmts(&arm.body, n);
                }
                if let Some(b) = else_body {
                    count_spawn_sites_stmts(b, n);
                }
            }
            TStmt::MixedSwitch {
                arms, else_body, ..
            } => {
                for (_, b) in arms {
                    count_spawn_sites_stmts(b, n);
                }
                if let Some(b) = else_body {
                    count_spawn_sites_stmts(b, n);
                }
            }
            _ => {}
        }
    }
}

fn count_spawn_sites_expr(expr: &TExpr, n: &mut SpawnSiteTally) {
    // TIR assigned these indices, and the lowering reads them off the node
    // (`LowerCtx::lower_spawn`, `lower_spawn_site_cb_at`), so the SAME index
    // referenced twice is still one table entry.
    if let TExprKind::CoreClosureCall {
        kind: TCoreClosureKind::Spawn { site, .. }
            | TCoreClosureKind::ReactiveDerived { site, .. }
            | TCoreClosureKind::ReactiveEffect { site, .. }
            | TCoreClosureKind::UiReactiveRender { site, .. }
            | TCoreClosureKind::UiButtonOnClick { site, .. },
    } = &expr.kind
    {
        n.assigned_site(*site);
    }
    if let TExprKind::HandleMethod {
        op: THandleOp::WatchMethod {
            callback_index: Some(site),
            ..
        },
        ..
    } = &expr.kind
    {
        n.assigned_site(*site);
    }
    // The rest have no site on their TIR node yet, so each occurrence takes the
    // next cursor slot (`LowerCtx::lower_spawn_site_cb`).
    if matches!(
        &expr.kind,
        TExprKind::HandleMethod {
            op: THandleOp::GameSceneOnFrame,
            ..
        }
    ) {
        n.cursor_slot();
    }
    if let TExprKind::HandleMethod {
        op: THandleOp::EventMethod { method },
        ..
    } = &expr.kind
    {
        if matches!(method.as_str(), "on" | "once" | "on_priority") {
            n.cursor_slot();
        }
    }
    if let TExprKind::HandleMethod {
        op: THandleOp::UiBackendMethod { method },
        ..
    } = &expr.kind
    {
        if method == "on_click" {
            n.cursor_slot();
        }
    }
    match &expr.kind {
        TExprKind::Print(inner)
        | TExprKind::Unary { operand: inner, .. }
        | TExprKind::Clone(inner)
        | TExprKind::ExplicitCopy(inner)
        | TExprKind::Ok(inner)
        | TExprKind::Err(inner) => count_spawn_sites_expr(inner, n),
        TExprKind::ListLit(elems) | TExprKind::ColumnarListLit { elems, .. } => {
            for elem in elems {
                count_spawn_sites_expr(elem, n);
            }
        }
        TExprKind::ListSpread { parts } => {
            for part in parts {
                let expr = match part {
                    ListSpreadPart::Elem(expr) | ListSpreadPart::Spread(expr) => expr,
                };
                count_spawn_sites_expr(expr, n);
            }
        }
        TExprKind::Binary { lhs, rhs, .. } => {
            count_spawn_sites_expr(lhs, n);
            count_spawn_sites_expr(rhs, n);
        }
        TExprKind::Call { args, .. } => {
            for a in args {
                count_spawn_sites_expr(&a.value, n);
            }
        }
        TExprKind::IfExpr {
            cond,
            then_body,
            then_value,
            else_body,
            else_value,
            ..
        } => {
            count_spawn_sites_if_cond(cond, n);
            count_spawn_sites_stmts(then_body, n);
            count_spawn_sites_expr(then_value, n);
            count_spawn_sites_stmts(else_body, n);
            count_spawn_sites_expr(else_value, n);
        }
        TExprKind::HandleMethod { recv, args, .. } => {
            count_spawn_sites_expr(recv, n);
            for a in args {
                count_spawn_sites_expr(a, n);
            }
        }
        TExprKind::BuiltinMethod { recv, args, .. }
        | TExprKind::ClosureMethod { recv, args, .. } => {
            count_spawn_sites_expr(recv, n);
            for a in args {
                count_spawn_sites_expr(a, n);
            }
        }
        TExprKind::CoreCall { args, .. } => {
            for a in args {
                count_spawn_sites_expr(a, n);
            }
        }
        TExprKind::OrFallback { value, .. } => count_spawn_sites_expr(value, n),
        TExprKind::TaskGroupAll { tasks }
        | TExprKind::TaskGroupRace { tasks }
        | TExprKind::TaskGroupAny { tasks } => count_spawn_sites_expr(tasks, n),
        _ => {}
    }
}

/// Return the first global callback site referenced by a nested spawn lambda.
///
/// Top-level functions share one traversal cursor. A nested lambda has its own
/// compiler pass, so it must start that cursor at the first site in its body;
/// otherwise the second nested task/combinator resolves to the first lambda's
/// callback. The TIR site is already authoritative; this helper only locates
/// it for the JIT adapter.
pub(crate) fn first_spawn_site(lambda: &TJitSpawnLambda) -> Option<usize> {
    fn find_expr(node: &TExpr) -> Option<usize> {
        match &node.kind {
            TExprKind::CoreClosureCall {
                kind: TCoreClosureKind::Spawn { site, .. }
                    | TCoreClosureKind::ReactiveDerived { site, .. }
                    | TCoreClosureKind::ReactiveEffect { site, .. }
                    | TCoreClosureKind::UiReactiveRender { site, .. }
                    | TCoreClosureKind::UiButtonOnClick { site, .. },
            } => Some(*site),
            TExprKind::Print(inner)
            | TExprKind::Unary { operand: inner, .. }
            | TExprKind::Clone(inner)
            | TExprKind::Ok(inner)
            | TExprKind::Err(inner)
            | TExprKind::Try { inner, .. } => find_expr(inner),
            TExprKind::Binary { lhs, rhs, .. } => find_expr(lhs).or_else(|| find_expr(rhs)),
            TExprKind::Call { args, .. } => args.iter().find_map(|arg| find_expr(&arg.value)),
            TExprKind::HandleMethod { recv, args, .. }
            | TExprKind::BuiltinMethod { recv, args, .. }
            | TExprKind::ClosureMethod { recv, args, .. } => find_expr(recv)
                .or_else(|| args.iter().find_map(find_expr)),
            TExprKind::CoreCall { args, .. } => args.iter().find_map(find_expr),
            TExprKind::OrFallback { value, fallback } => find_expr(value).or_else(|| match fallback {
                TOrFallback::Value(inner) | TOrFallback::Return(Some(inner)) => find_expr(inner),
                TOrFallback::Panic { msg, .. } => find_expr(msg),
                TOrFallback::Return(None)
                | TOrFallback::Break
                | TOrFallback::Continue
                | TOrFallback::BreakLabel(_)
                | TOrFallback::ContinueLabel(_) => None,
            }),
            TExprKind::ListLit(elems) => elems.iter().find_map(find_expr),
            TExprKind::TaskGroupAll { tasks }
            | TExprKind::TaskGroupRace { tasks }
            | TExprKind::TaskGroupAny { tasks } => find_expr(tasks),
            _ => None,
        }
    }
    fn find_stmts(stmts: &[TStmt]) -> Option<usize> {
        stmts.iter().find_map(|stmt| match stmt {
            TStmt::Let { init, .. }
            | TStmt::Assign { value: init, .. }
            | TStmt::ExprStmt(init)
            | TStmt::Return(Some(init)) => find_expr(init),
            TStmt::TaskGroup { body, .. }
            | TStmt::Region(body)
            | TStmt::Impure(body)
            | TStmt::Inline(body)
            | TStmt::Unsafe { body, .. }
            | TStmt::SentryPolicy { body, .. }
            | TStmt::Shield { body }
            | TStmt::DebugOnly(body)
            | TStmt::Layout { body, .. } => find_stmts(body),
            TStmt::If {
                then_body,
                else_body,
                ..
            } => find_stmts(then_body).or_else(|| else_body.as_deref().and_then(find_stmts)),
            TStmt::EnumMatch {
                scrutinee,
                arms,
                else_body,
                ..
            } => find_expr(scrutinee)
                .or_else(|| arms.iter().find_map(|arm| find_stmts(&arm.body)))
                .or_else(|| else_body.as_deref().and_then(find_stmts)),
            _ => None,
        })
    }
    match &lambda.body {
        TJitSpawnBody::Expr(expr) => find_expr(expr),
        TJitSpawnBody::Block { prefix, tail } => {
            find_stmts(prefix).or_else(|| tail.as_deref().and_then(find_expr))
        }
        TJitSpawnBody::SharedBlock { body, .. } => find_stmts(body),
    }
}

fn count_spawn_sites_if_cond(cond: &TIfCond, n: &mut SpawnSiteTally) {
    match cond {
        TIfCond::Plain(expr) => count_spawn_sites_expr(expr, n),
        TIfCond::And { left, right } => {
            count_spawn_sites_if_cond(left, n);
            count_spawn_sites_if_cond(right, n);
        }
        TIfCond::IfLet { subj, .. }
        | TIfCond::IsNone { subj }
        | TIfCond::Matches { subj, .. } => count_spawn_sites_expr(subj, n),
    }
}

fn resident_safe_expr_list(exprs: &[TExpr], callees: &HashSet<String>) -> bool {
    exprs.iter().all(|e| resident_safe_expr(e, callees))
}

fn resident_safe_task_list_expr(tasks: &TExpr, callees: &HashSet<String>) -> bool {
    jit_list_task_type(&tasks.ty) && resident_safe_expr(tasks, callees)
}

fn resident_safe_select_wait(builder: &TExpr, callees: &HashSet<String>) -> bool {
    let (recvs, afters) = collect_select_arms_jit(builder);
    (!recvs.is_empty() || !afters.is_empty())
        && recvs
            .iter()
            .all(|ch| jit_concurrency_type(&ch.ty) && resident_safe_expr(ch, callees))
        && afters.iter().all(|(duration, value)| {
            matches!(&duration.ty, Type::Named(name) if name == "Duration")
                && resident_safe_expr(duration, callees)
                && value
                    .map(|v| matches!(&v.ty, Type::Int) && resident_safe_expr(v, callees))
                    .unwrap_or(true)
        })
}

pub(crate) fn collect_select_arms_jit<'a>(
    builder: &'a TExpr,
) -> (Vec<&'a TExpr>, Vec<(&'a TExpr, Option<&'a TExpr>)>) {
    let mut recvs = Vec::new();
    let mut afters = Vec::new();
    let mut cur = builder;
    loop {
        match &cur.kind {
            TExprKind::SelectStart => break,
            TExprKind::SelectRecv {
                builder: inner,
                channel,
            } => {
                recvs.push(channel.as_ref());
                cur = inner;
            }
            TExprKind::SelectAfter {
                builder: inner,
                duration,
                value,
            } => {
                afters.push((duration.as_ref(), value.as_deref()));
                cur = inner;
            }
            _ => break,
        }
    }
    (recvs, afters)
}

pub(crate) fn resident_safe_spawn_lambda(lam: &TJitSpawnLambda, callees: &HashSet<String>) -> bool {
    if lam.captures.len() > 4 {
        return false;
    }
    if !lam
        .captures
        .iter()
        .all(|c| jit_value_type(&c.ty) && resident_safe_capture_policy(c))
    {
        return false;
    }
    if !lam.params.iter().all(|(_, ty)| jit_value_type(ty)) {
        return false;
    }
    if !jit_value_type(&lam.ret) {
        return false;
    }
    match &lam.body {
        TJitSpawnBody::Expr(e) => resident_safe_expr(e, callees),
        TJitSpawnBody::Block { prefix, tail } => {
            prefix.iter().all(|s| resident_safe_stmt(s, callees))
                && tail.as_ref().is_none_or(|t| resident_safe_expr(t, callees))
        }
        TJitSpawnBody::SharedBlock { body, tail } => {
            body.iter().all(|s| resident_safe_stmt(s, callees))
                && (!tail
                    || body
                        .last()
                        .and_then(|stmt| match stmt {
                            TStmt::ExprStmt(expr) | TStmt::Return(Some(expr)) => Some(expr),
                            _ => None,
                        })
                        .is_some_and(|expr| resident_safe_expr(expr, callees)))
        }
    }
}

pub(crate) fn resident_safe_capture_policy(c: &JitSpawnCapture) -> bool {
    if c.materialize_at_spawn {
        // D-MEM-COPYSEM1=A: read-view captures have already crossed to their
        // owning ABI type. The resident path supports the same list/string
        // clone doors as ordinary TIR ownership lowering.
        return matches!(&c.ty, Type::String)
            || jit_list_native_type(&c.ty)
            || jit_list_of_int_list_type(&c.ty)
            || jit_list_record_type(&c.ty);
    }
    if c.frozen_at_spawn {
        // D-CONC-FREEZE1=A: sema's `check_freeze` proved this capture is a
        // deeply immutable, deeply cloneable owned snapshot, and the proof
        // rides TIR as `frozen_at_spawn`. The spawn clone lowers through the
        // same `lower_clone` record/list/map/tuple/string doors an ordinary
        // `Clone` node uses — the engine marshals the fact and never re-runs
        // crossing policy (I9). Same door set as the call-arg `clone_ok` gate.
        return matches!(
            &c.ty,
            Type::Int
                | Type::Float
                | Type::Bool
                | Type::Char
                | Type::String
                | Type::Option(_)
                | Type::IntN { .. }
                | Type::Float32
        ) || jit_struct_type(&c.ty)
            || jit_compound_type(&c.ty)
            || jit_tuple_type(&c.ty)
            || jit_list_native_type(&c.ty)
            || jit_list_record_type(&c.ty)
            || jit_map_string_type(&c.ty);
    }
    if c.clone_at_spawn {
        // Sender and Receiver are Arc-backed handles (Prelude Clone); spawn
        // capture clones the handle id the same way AOT clones JetSender /
        // JetReceiver (D-TUPLE-DESTRUCT1).
        matches!(&c.ty, Type::Apply { name, .. } if matches!(name.as_str(), "Sender" | "Receiver"))
            || matches!(&c.ty, Type::Shared(_))
            || matches!(
                &c.ty,
                Type::String
                    | Type::Int
                    | Type::IntN { .. }
                    | Type::Float
                    | Type::Float32
                    | Type::Bool
                    | Type::Char
            )
            || opaque_host_handle_ty(&c.ty)
    } else {
        true
    }
}

/// Resident JIT opaque handles: i64 slots, clone = copy.
pub(crate) fn opaque_host_handle_ty(ty: &Type) -> bool {
    match ty {
        Type::Named(n) => matches!(
            n.as_str(),
            "NullBackend"
                | "TuiBackend"
                | "GtkBackend"
                | "UiNode"
                | "EventResult"
                | "InputEvent"
                | "AriaRole"
                | "Point"
                | "Size"
                | "Rect"
                | "SizeConstraint"
                | "App"
                | "WebPage"
                | "DevServer"
                | "AsyncPolicy"
                | "Overflow"
                | "FailurePolicy"
                | "DispatchState"
                | "HookPolicy"
                | "HookDecision"
                | "HookOutcome"
                | "EventConfigError"
                | "Layout"
                | "Mod"
                | "GameScene"
                | "GameFrame"
                | "GameBackend"
                | "Condition"
                | "RaylibWindow"
                | "RaylibColor"
                | "RaylibSound"
                | "TcpListener"
                | "TcpStream"
                | "TLSStream"
                | "TLSClientConfig"
                | "TLSRootCertificates"
                | "TLSClientIdentity"
                | "SocketAddr"
                | "UdpSocket"
                | "UnixListener"
                | "UnixStream"
                | "HTTPMux"
                | "HTTPServer"
                | "HTTPHandler"
                | "HTTPRequest"
                | "HTTPResponse"
                | "HTTPBody"
                | "HTTPHeaders"
                | "HTTPMethod"
                | "HTTPStatus"
                | "HTTPVersion"
                | "HTTPHeaderName"
                | "HTTPHeaderValue"
                | "HTTPCorsPolicy"
                | "WsConn"
                | "WsMessage"
        ),
        Type::Apply { name, .. } => matches!(
            name.as_str(),
            "Signal"
                | "Derived"
                | "Computed"
                | "Effect"
                | "Loadable"
                | "Event"
                | "AsyncEvent"
                | "Hook"
                | "DecisionHook"
                | "Watch"
        ),
        _ => false,
    }
}

/// Body of `child.stdout.lines()` — loop var type is erased to Unit in TIR.
fn resident_safe_process_lines_body(stmt: &TStmt, callees: &HashSet<String>) -> bool {
    match stmt {
        TStmt::ExprStmt(e) => match &e.kind {
            TExprKind::Print(inner) => match &inner.kind {
                TExprKind::BuiltinMethod { op, recv, args }
                    if matches!(
                        op,
                        TBuiltinOp::Trim | TBuiltinOp::ToUpper | TBuiltinOp::ToLower
                    ) && args.is_empty()
                        && matches!(&recv.kind, TExprKind::Local(_)) =>
                {
                    true
                }
                TExprKind::Local(_) => true,
                _ => resident_safe_expr(e, callees),
            },
            TExprKind::Try { inner, .. } => match &inner.kind {
                TExprKind::HandleMethod {
                    op: THandleOp::FileWriterWriteLine,
                    args,
                    ..
                } if args.len() == 1 => match &args[0].kind {
                    TExprKind::BuiltinMethod { op, recv, args: a }
                        if matches!(op, TBuiltinOp::ToUpper | TBuiltinOp::ToLower | TBuiltinOp::Trim)
                            && a.is_empty()
                            && matches!(&recv.kind, TExprKind::Local(_)) =>
                    {
                        true
                    }
                    TExprKind::Local(_) => true,
                    _ => resident_safe_expr(inner, callees),
                },
                _ => resident_safe_expr(e, callees),
            },
            TExprKind::HandleMethod {
                op: THandleOp::FileWriterWriteLine,
                args,
                ..
            } if args.len() == 1 => match &args[0].kind {
                TExprKind::BuiltinMethod { op, recv, args: a }
                    if matches!(op, TBuiltinOp::ToUpper | TBuiltinOp::ToLower | TBuiltinOp::Trim)
                        && a.is_empty()
                        && matches!(&recv.kind, TExprKind::Local(_)) =>
                {
                    true
                }
                TExprKind::Local(_) => true,
                _ => resident_safe_expr(e, callees),
            },
            _ => resident_safe_expr(e, callees),
        },
        TStmt::Assign { value, .. } => resident_safe_expr(value, callees),
        TStmt::If {
            cond,
            then_body,
            else_body,
            ..
        } => {
            let cond_ok = match cond {
                TIfCond::Plain(e) => match &e.kind {
                    TExprKind::BuiltinMethod { op, recv, args }
                        if matches!(
                            op,
                            TBuiltinOp::Contains | TBuiltinOp::StartsWith | TBuiltinOp::EndsWith
                        ) && args.len() == 1
                            && matches!(&recv.kind, TExprKind::Local(_))
                            && matches!(&args[0].ty, Type::String)
                            && resident_safe_expr(&args[0], callees) =>
                    {
                        true
                    }
                    _ => matches!(&e.ty, Type::Bool) && resident_safe_expr(e, callees),
                },
                _ => false,
            };
            cond_ok
                && then_body
                    .iter()
                    .all(|s| resident_safe_process_lines_body(s, callees))
                && else_body.as_ref().is_none_or(|b| {
                    b.iter()
                        .all(|s| resident_safe_process_lines_body(s, callees))
                })
        }
        _ => resident_safe_stmt(stmt, callees),
    }
}

fn resident_safe_handle_op(op: &THandleOp, recv: &TExpr, args: &[TExpr]) -> bool {
    match op {
        THandleOp::TaskJoin
        | THandleOp::TaskCancel
        | THandleOp::TaskDetach
        | THandleOp::TaskPause
        | THandleOp::TaskResume => {
            args.is_empty() && jit_concurrency_type(&recv.ty)
        }
        THandleOp::ChannelReceive => {
            args.is_empty() && matches!(&recv.ty, Type::Apply { name, .. } if name == "Receiver")
        }
        THandleOp::ChannelClose => {
            args.is_empty()
                && matches!(
                    &recv.ty,
                    Type::Apply { name, .. } if matches!(name.as_str(), "Receiver" | "Sender")
                )
        }
        THandleOp::SenderSend => {
            args.len() == 1 && matches!(&recv.ty, Type::Apply { name, .. } if name == "Sender")
        }
        THandleOp::FakeLocale => {
            args.len() == 1 && recv.ty == Type::Named("Fake".into()) && args[0].ty == Type::String
        }
        THandleOp::FakeName
        | THandleOp::FakeEmail
        | THandleOp::FakeHost
        | THandleOp::FakeAddress => {
            args.is_empty() && recv.ty == Type::Named("Fake".into())
        }
        THandleOp::SolverNew => args.is_empty() && recv.ty == Type::Int,
        THandleOp::SolverRequire => args.len() == 1 && recv.ty == Type::Named("Solver".into()) && args[0].ty == Type::Bool,
        THandleOp::SolverFailureCount | THandleOp::SolverStatus => {
            args.is_empty() && recv.ty == Type::Named("Solver".into())
        }
        THandleOp::MeasurementMethod { method } => {
            matches!(&recv.ty, Type::Apply { name, args: targs }
                if name == "Measurement" && targs.as_slice() == [Type::Float])
                && matches!(
                    (method.as_str(), args.len()),
                    ("value" | "uncertainty" | "sqrt", 0)
                        | ("add" | "sub" | "mul" | "div", 1)
                )
        }
        THandleOp::PreciseMethod { type_name, method } => {
            (type_name == "Decimal"
                    && recv.ty == Type::Named("Decimal".into())
                    && matches!(
                        (method.as_str(), args.len()),
                        ("add" | "sub" | "mul" | "equal", 1) | ("to_string", 0)
                    ))
                || (type_name == "Fraction"
                    && recv.ty == Type::Named("Fraction".into())
                    && matches!(
                        (method.as_str(), args.len()),
                        ("add" | "sub" | "mul" | "div" | "equal", 1)
                            | (
                                "numerator"
                                    | "denominator"
                                    | "to_string"
                                    | "to_float"
                                    | "is_zero",
                                0
                            )
                    )) 
        }
        THandleOp::CivilTimeMethod { kind, method } => {
            // Civil date/time/zoned methods are host-backed; arity is checked in lower.
            //
            // I8: ask the one table lowering built this node from, never a second
            // flattened copy. The copy that used to live here dropped `kind`, so
            // it admitted `ZonedDateTime.add_duration`/`add_period` — which had no
            // arm in `Time.rs`'s dispatch — and the host answered with a null
            // carrier handle the program then read as an unrelated heap slot
            // (#2030). It also refused `Zone.name`, which the host does marshal,
            // and carried three names (`timestamp`, `add_years`, `with_time`) no
            // receiver ever offers.
            matches!(args.len(), 0..=6)
                && TIR::is_civil_time_method_name(Some(kind.as_str()), method)
        }
        THandleOp::RegexMethod { method, .. } => {
            matches!(
                (method.as_str(), args.len()),
                (
                    "matches"
                        | "match"
                        | "is_match"
                        | "full_match"
                        | "find"
                        | "find_all"
                        | "split"
                        | "start"
                        | "end"
                        | "pattern"
                        | "source"
                        | "flags"
                        | "options"
                        | "names"
                        | "named_captures"
                        | "count",
                    0..=1
                ) | ("group" | "name", 1)
                    | ("replace_all" | "split_limit" | "replace_all_with", 2)
            )
        }
        THandleOp::FileReaderReadLine if args.is_empty() => true,
        THandleOp::FileWriterWriteLine if args.len() == 1 => true,
        THandleOp::FileWriterFlush if args.is_empty() => true,
        THandleOp::StdinReadLine if args.is_empty() => true,
        THandleOp::StdoutWrite
        | THandleOp::StdoutWriteLine
        | THandleOp::StdoutWriteBytes
            if args.len() == 1 =>
        {
            true
        }
        THandleOp::StdoutFlush | THandleOp::StdoutIsTty if args.is_empty() => true,
        THandleOp::StderrWrite
        | THandleOp::StderrWriteLine
        | THandleOp::StderrWriteBytes
            if args.len() == 1 =>
        {
            true
        }
        THandleOp::StderrFlush | THandleOp::StderrIsTty if args.is_empty() => true,
        THandleOp::DBWithPolicy
        | THandleOp::DBQuery
        | THandleOp::DBQueryOne
        | THandleOp::DBExecute
            if args.len() == 2 => true,
        THandleOp::DBBegin
        | THandleOp::DBCommit
        | THandleOp::DBRollback
        | THandleOp::DBClose
        | THandleOp::DBValueInt
        | THandleOp::DBValueFloat
        | THandleOp::DBValueText
        | THandleOp::DBValueBool
        | THandleOp::DBValueIsNull
            if args.is_empty() =>
        {
            true
        }
        THandleOp::ServiceRuntimeSend => {
            args.len() == 3 && recv.ty == Type::Named("ServiceRuntime".into())
        }
        THandleOp::ServiceRuntimeRetry
        | THandleOp::ServiceRuntimeDeadLetter
        | THandleOp::ServiceRuntimeRetain
        | THandleOp::ServiceRuntimeCommit => {
            args.len() == 1 && recv.ty == Type::Named("ServiceRuntime".into())
        }
        THandleOp::DurationNew { .. } => args.is_empty(),
        THandleOp::DurationIn { .. } => args.len() <= 1,
        THandleOp::DurationIsZero | THandleOp::DurationTotalSeconds => args.is_empty(),
        THandleOp::DurationDifference => args.len() == 1,
        THandleOp::DurationSecondsValue => args.is_empty(),
        THandleOp::AllocAlloc | THandleOp::AllocTryAlloc => args.len() == 1,
        THandleOp::AllocReset
        | THandleOp::ClockNow
        | THandleOp::StopwatchElapsedMillis
        | THandleOp::TestSuiteRun
        | THandleOp::RngBool
        | THandleOp::RngFloat
        | THandleOp::RngSplit => args.is_empty(),
        THandleOp::RngInt => args.len() == 2,
        THandleOp::RngFloatRange
        | THandleOp::RngNormal
        | THandleOp::RngWeightedPick
        | THandleOp::RngSample => {
            args.len() == 2
        }
        THandleOp::RngBoolP | THandleOp::RngExponential | THandleOp::RngBytes => {
            args.len() == 1
        }
        THandleOp::ClockTick
        | THandleOp::ClockAdvance
        | THandleOp::ClockWait
        | THandleOp::RngPick
        | THandleOp::RngShuffle => args.len() == 1,
        THandleOp::ExpiringMethod { method } => {
            matches!(method.as_str(), "get" | "is_valid") && args.len() == 1
        }
        THandleOp::ProcessSpecMethod { method } => {
            matches!(
                (method.as_str(), args.len()),
                (
                    "stdout" | "stderr" | "stdin" | "timeout" | "output_limit" | "cwd"
                        | "env_remove",
                    1,
                ) | ("env", 2)
                    | ("terminal", 0 | 1)
                    | (
                        "env_clear" | "detached" | "abilities" | "plan" | "run" | "run_checked"
                            | "spawn",
                        0,
                    )
                    | ("under", 1)
            )
        }
        THandleOp::ProcessChildMethod { method } => {
            matches!(
                (method.as_str(), args.len()),
                ("wait" | "id" | "exited" | "kill" | "terminate" | "interrupt", 0)
            )
        }
        THandleOp::TerminalSessionResize => args.len() == 1,
        THandleOp::EventMethod { method } => {
            matches!(
                (method.as_str(), args.len()),
                ("cancel" | "is_active", 0)
            )
        }
        // Keep residency aligned with the actual LowerCtx HTTP arms below.
        // The evaluator has a separate ambient bridge for the subset field
        // forms; those stay deopt-only until LowerCtx grows matching ABI arms.
        THandleOp::HTTPClientMethod { kind, method } => match (kind.as_str(), method.as_str()) {
            ("HTTPResponse", "status" | "body" | "cookies") if args.is_empty() => true,
            ("HTTPResponse", "header") if args.len() == 1 => true,
            ("HTTPResponse", "json") if args.len() <= 1 => true,
            ("HTTPBody", "text" | "json" | "bytes") if args.len() == 1 => true,
            ("HTTPBody", "copy_to") if args.len() == 2 => true,
            ("HTTPRequest", "body") if args.len() == 1 => true,
            ("HTTPRequest", "form" | "cookie" | "header") if args.len() == 2 => true,
            ("HTTPRequest", "redirects" | "connect_timeout" | "read_timeout")
                if args.len() == 1 => true,
            ("HTTPRequest", "send") if args.is_empty() => true,
            _ => false,
        },
        THandleOp::HTTPServerMethod { kind, method } => match (kind.as_str(), method.as_str()) {
            ("HTTPMux", m)
                if matches!(
                    m,
                    "get" | "post" | "put" | "delete" | "patch" | "head" | "options"
                ) && args.len() == 2 => true,
            ("HTTPMux", "middleware") if args.len() == 1 => true,
            ("HTTPHandler", "handle") if args.len() == 1 => true,
            ("HTTPRequest", "body" | "method" | "path" | "trailers" | "body_len" | "json")
                if args.is_empty() => true,
            ("HTTPRequest", "param" | "header" | "under_limit") if args.len() == 1 => true,
            ("HTTPBody", "text" | "json" | "bytes") if args.len() == 1 => true,
            ("HTTPBody", "copy_to") if args.len() == 2 => true,
            ("HTTPResponse", "status" | "body") if args.is_empty() => true,
            ("HTTPResponse", "trailers") if args.len() == 1 => true,
            ("HTTPServer", "local_addr" | "serve") if args.is_empty() => true,
            ("HTTPServer", "shutdown") if args.len() == 1 => true,
            ("WsConn", "recv") if args.is_empty() => true,
            ("WsConn", "send_text") if args.len() == 1 => true,
            ("WsConn", "close") if args.len() == 2 => true,
            ("WsMessage", "is_text" | "text") if args.is_empty() => true,
            _ => false,
        },
        THandleOp::HTTPReqTrailers => args.is_empty(),
        THandleOp::HTTPRespTrailers => args.len() == 1,
        THandleOp::WatchMethod { method, .. } => match method.as_str() {
            "poll" | "events" | "cancel" | "is_active" | "summary" => args.is_empty(),
            "add" => args.len() == 1,
            "on" | "once" => args.len() == 2,
            _ => false,
        },
        THandleOp::ArgsSpecFlag
        | THandleOp::ArgsSpecPositional
            if args.len() == 2 =>
        {
            true
        }
        THandleOp::ArgsSpecFlagShort
        | THandleOp::ArgsSpecOption
        | THandleOp::ArgsSpecOptionInt
        | THandleOp::ArgsSpecRepeat
        | THandleOp::ArgsSpecSubcommand
            if args.len() == 3 =>
        {
            true
        }
        THandleOp::ArgsSpecOptionDefault | THandleOp::ArgsSpecOptionChoice if args.len() == 4 => {
            true
        }
        THandleOp::ArgsSpecVersion
        | THandleOp::ArgsSpecCompletion
        | THandleOp::ArgsSpecDescription
        | THandleOp::ArgsSpecParse
        | THandleOp::ArgsSpecParseOrExit
            if args.len() == 1 =>
        {
            true
        }
        THandleOp::ArgsSpecHelp if args.is_empty() => true,
        THandleOp::ParsedArgsFlag
        | THandleOp::ParsedArgsOption
        | THandleOp::ParsedArgsOptionInt
        | THandleOp::ParsedArgsOptionFloat
        | THandleOp::ParsedArgsOptions
        | THandleOp::ParsedArgsPositional
            if args.len() == 1 =>
        {
            true
        }
        THandleOp::ParsedArgsSubcommand if args.is_empty() => true,
        THandleOp::SketchMethod { sketch, method } => match method.as_str() {
            "add" => args.len() == 1,
            "count" if sketch == "HyperLogLog" => args.is_empty(),
            "count" if sketch == "CountMinSketch" => args.len() == 1,
            "quantile" => args.len() == 1,
            "sample" => args.is_empty(),
            _ => false,
        },
        THandleOp::DataTreeField
        | THandleOp::JSONField
        | THandleOp::DataTreeAt
        | THandleOp::JSONAt => args.len() == 1,
        THandleOp::DataTreeInt
        | THandleOp::JSONInt
        | THandleOp::DataTreeText
        | THandleOp::JSONText
        | THandleOp::DataTreeBool
        | THandleOp::JSONBool
        | THandleOp::DataTreeFloat
        | THandleOp::JSONFloat => args.is_empty(),
        THandleOp::SerdeEncode => args.is_empty(),
        THandleOp::DataTreeDecode(_) => args.is_empty(),
        THandleOp::JSONReaderNext
        | THandleOp::JSONLReaderNext
        | THandleOp::CSVReaderNext
        | THandleOp::DataStreamNext
        | THandleOp::XMLReaderNext
        | THandleOp::CBORReaderNext
        | THandleOp::JSONWriterFlush
        | THandleOp::JSONWriterFinish
        | THandleOp::JSONLWriterFlush
        | THandleOp::JSONLWriterFinish
        | THandleOp::CSVWriterFlush
        | THandleOp::CSVWriterFinish
        | THandleOp::XMLWriterFlush
        | THandleOp::XMLWriterFinish
        | THandleOp::CBORWriterFlush
        | THandleOp::CBORWriterFinish => args.is_empty(),
        THandleOp::JSONWriterWrite
        | THandleOp::JSONLWriterWrite
        | THandleOp::CSVWriterWrite
        | THandleOp::XMLWriterWrite
        | THandleOp::CBORWriterWrite => args.len() == 1,
        THandleOp::PathFrom => matches!(&recv.ty, Type::String) && args.is_empty(),
        THandleOp::PathHome => args.is_empty(),
        THandleOp::PathJoin => args.len() == 1 && matches!(&args[0].ty, Type::String),
        THandleOp::PathParent
        | THandleOp::PathExtension
        | THandleOp::PathStem
        | THandleOp::PathNormalize
        | THandleOp::PathToString
        |         THandleOp::PathWalk => args.is_empty(),
        THandleOp::PathWriteAtomic => args.len() == 1,
        THandleOp::GameSceneNew => args.is_empty() && matches!(&recv.ty, Type::String),
        THandleOp::GameReplayRecord => args.is_empty() && matches!(&recv.ty, Type::String),
        THandleOp::GameBackendHeadless => args.is_empty(),
        THandleOp::GameBackendShouldContinue | THandleOp::GameBackendPresent => args.is_empty(),
        THandleOp::GameSceneOnFrame => {
            // AOT keeps the frame lambda in TIR args; JIT registers via spawn-site.
            args.len() <= 1
        }
        THandleOp::GameSceneComponent | THandleOp::GameSceneQuery => {
            args.len() == 1 && matches!(&args[0].ty, Type::String)
        }
        THandleOp::GameAssetsImage | THandleOp::GameAssetsSound => {
            args.len() == 1 && matches!(&args[0].ty, Type::String)
        }
        THandleOp::GameInputBind => {
            args.len() == 2
                && matches!(&args[0].ty, Type::String)
                && matches!(&args[1].ty, Type::String)
        }
        THandleOp::GameInputPressed => args.len() == 1 && matches!(&args[0].ty, Type::String),
        THandleOp::ReactiveGet => args.is_empty(),
        THandleOp::ReactiveSet => args.len() == 1,
        THandleOp::ReactiveEffectMethod { .. } => args.is_empty(),
        THandleOp::LayoutMethod { .. } => true,
        THandleOp::LoadableMethod { .. } => true,
        THandleOp::UiBackendMethod { .. } => true,
        THandleOp::DevServerMethod { .. } => true,
        THandleOp::AppMethod { .. } => true,
        THandleOp::ReaderOver
        | THandleOp::ReaderReadU8
        | THandleOp::ReaderReadU16Le
        | THandleOp::ReaderReadU16Be
        | THandleOp::ReaderReadU32Le
        | THandleOp::ReaderReadU32Be
        | THandleOp::ReaderReadU64Le
        | THandleOp::ReaderReadU64Be
        | THandleOp::ReaderRemaining
        | THandleOp::ReaderAtEnd
        | THandleOp::CursorOver
        | THandleOp::CursorSkipWs => args.is_empty(),
        THandleOp::ReaderTake | THandleOp::CursorTakeUntil => args.len() == 1,
        THandleOp::CursorTakePattern { .. } | THandleOp::ReaderTakePattern { .. } => {
            args.is_empty()
        }
        THandleOp::ReflectValueTypeName
        | THandleOp::ReflectValuePath
        | THandleOp::ReflectValueDisplay
        | THandleOp::ReflectValueFields
        | THandleOp::ReflectFieldName
        | THandleOp::ReflectFieldValue => args.is_empty(),
        THandleOp::UrlMimeMethod { method, .. } => match method.as_str() {
            "to_string" | "scheme" | "host" | "port" | "path" | "query" | "query_pairs"
            | "path_segments" | "fragment" | "essence" | "username" | "password"
            | "userinfo" | "authority" | "default_port" | "normalize"
                if args.is_empty() =>
            {
                true
            }
            "join" | "param" if args.len() == 1 => true,
            "set_query" | "add_query" if args.len() == 2 => true,
            _ => false,
        },
        THandleOp::TcpListenerAccept
        | THandleOp::TcpListenerLocalAddr
        | THandleOp::TcpStreamClose
        | THandleOp::UdpSocketClose => args.is_empty(),
        THandleOp::TcpStreamReadText | THandleOp::TcpStreamWriteAllBytes if args.len() == 1 => true,
        THandleOp::TcpStreamReady | THandleOp::UdpSocketReady if args.len() == 2 => true,
        THandleOp::UdpSocketReceiveDeadline if args.len() == 2 => true,
        THandleOp::UdpSocketSendToDeadline if args.len() == 3 => true,
        THandleOp::TLSStreamReadDeadline
        | THandleOp::TLSStreamWriteAllDeadline
        | THandleOp::TLSStreamReady if args.len() == 2 => true,
        THandleOp::TLSStreamClose | THandleOp::TLSStreamPeerIdentity if args.is_empty() => true,
        THandleOp::TLSStreamCloseWrite if args.len() == 1 => true,
        THandleOp::TLSClientConfigDefault if args.is_empty() => true,
        THandleOp::TLSClientConfigWithAlpn
        | THandleOp::TLSRootCertificatesFromPem
        | THandleOp::TLSClientConfigWithTrust
        | THandleOp::TLSClientConfigWithIdentity
            if args.len() == 1 =>
        {
            true
        }
        THandleOp::TLSClientIdentityFromPem
        | THandleOp::TLSClientConfigWithVersionBounds if args.len() == 2 => true,
        THandleOp::MathMethod { .. } => true,
        _ => false,
    }
}
