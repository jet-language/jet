use crate::jet_generated_format as jet_format;
use crate::Codegen::mangle;
use crate::Codegen::mangle_generated;
use crate::Codegen::rust_param_type;
use crate::Codegen::rust_return_type;
use crate::Codegen::Cx;
use crate::Codegen::TIR::SerdeCodec;
use crate::Codegen::TIR::TFunc;
use crate::Codegen::TIR::TFuncKind;
use crate::Codegen::TIR::TStmt;
use crate::Codegen::TIR::{emit_tir_expr, emit_tir_stmts, TExpr, TExprKind, TLocal, TPurity};
use crate::AST::{AccessConvention, BinOp, Type, UnOp, ViewSource};

fn emit_stack_guard(tir: &TFunc, cx: &Cx, out: &mut String, indent: usize) {
    // A synthesized function keeps file/line/name attribution, but the source
    // line at that position belongs to the declaring item (a struct header, a
    // `#UnitFamily(...)` marker), not to this function — never embed it (I3:
    // markers erase in codegen).
    let source_line = if tir.synthetic {
        ""
    } else {
        cx.src
            .lines()
            .nth(tir.line.saturating_sub(1))
            .unwrap_or_default()
            .trim_end()
    };
    let pad = "    ".repeat(indent);
    out.push_str(&jet_name_format!(
        "{pad}let {name_prefix}stack_frame = crate::jet_stack_enter({}, {}, {}, {});\n",
        crate::Codegen::escape_rust_str(&cx.file),
        tir.line,
        crate::Codegen::escape_rust_str(&tir.name),
        crate::Codegen::escape_rust_str(source_line),
    ));
}

fn emit_sentry_frame(tir: &TFunc, cx: &Cx, out: &mut String, indent: usize) {
    if tir.uses_stack_sentry {
        // The Prelude owns stack-address liveness. This is a marshalling hook:
        // the engine installs one frame token for the TIR-proved lifetime and
        // does not duplicate sentry policy here (I9). It follows the gate so
        // fenced release code can activate the runtime witness before the
        // token checks whether instrumentation is available.
        out.push_str(&format!(
            "{}let _jet_sentry_frame = {}jet_mem::jet_sentry_frame();\n",
            "    ".repeat(indent),
            cx.root_prefix,
        ));
    }
}

fn emit_sentry_gate(tir: &TFunc, cx: &Cx, out: &mut String, indent: usize) {
    let Some(gate) = tir.unsafe_gate.as_ref() else {
        return;
    };
    let pad = "    ".repeat(indent);
    let scope = if gate.fenced {
        "jet_sentry_fenced_scope"
    } else {
        "jet_sentry_scope"
    };
    out.push_str(&format!(
        "{pad}let _jet_sentry = {}jet_mem::{scope}({}, {:?}, {}, {:?});\n",
        cx.root_prefix, gate.enabled, gate.file, gate.line, gate.reason,
    ));
}

#[derive(Clone, Copy)]
enum NativeFloat {
    F32,
    F64,
}

impl NativeFloat {
    fn from_type(ty: &Type) -> Option<Self> {
        match ty {
            Type::Float32 => Some(Self::F32),
            Type::Float => Some(Self::F64),
            _ => None,
        }
    }

    fn scalar_name(self) -> &'static str {
        match self {
            Self::F32 => "f32",
            Self::F64 => "f64",
        }
    }

    fn width(self, extent: usize) -> Option<usize> {
        match self {
            Self::F32 if extent >= 8 => Some(8),
            Self::F32 if extent >= 4 => Some(4),
            Self::F64 if extent >= 4 => Some(4),
            Self::F64 if extent >= 2 => Some(2),
            _ => None,
        }
    }

    fn max_width(self) -> usize {
        match self {
            Self::F32 => 8,
            Self::F64 => 4,
        }
    }
}

struct NativeVectorPlan<'a> {
    facts: &'a crate::AST::AutoVectorizationFacts,
    element: NativeFloat,
    width: usize,
    extent: Option<usize>,
    end: &'a TExpr,
    var: &'a str,
    body: &'a [TStmt],
    stores: Vec<(&'a TLocal, &'a TExpr)>,
}

fn same_native_local(left: &TLocal, right: &TLocal) -> bool {
    left.rust_name() == right.rust_name() && left.deref == right.deref
}

fn native_scalar_type(ty: &Type, element: NativeFloat) -> bool {
    matches!(
        (element, ty),
        (NativeFloat::F32, Type::Float32) | (NativeFloat::F64, Type::Float)
    )
}

fn native_vector_expr_supported(
    expr: &TExpr,
    var: &str,
    element: NativeFloat,
    extent: Option<usize>,
    outputs: &[&TLocal],
    current_output: &TLocal,
) -> bool {
    if !native_scalar_type(&expr.ty, element) {
        return false;
    }
    match &expr.kind {
        TExprKind::IntLit(..) | TExprKind::FloatLit(..) => true,
        TExprKind::Local(local) => {
            !local.is_persistent()
                && !local.uninit_scalar
                && !local.uninit_fixed
                && local.name != var
                && !outputs
                    .iter()
                    .any(|output| same_native_local(local, output))
        }
        TExprKind::Index {
            base,
            index,
            is_map,
            uninit_fixed,
            ..
        } => {
            if *is_map || *uninit_fixed {
                return false;
            }
            let TExprKind::Local(base_local) = &base.kind else {
                return false;
            };
            let TExprKind::Local(index_local) = &index.kind else {
                return false;
            };
            let elem = match &base.ty {
                Type::FixedList { elem, len } => {
                    let Some(length) = len.literal_value() else {
                        return false;
                    };
                    if usize::try_from(length).ok() != extent {
                        return false;
                    }
                    elem
                }
                Type::List(elem) => elem,
                _ => return false,
            };
            !base_local.is_persistent()
                && !base_local.uninit_fixed
                && !outputs.iter().any(|output| {
                    same_native_local(base_local, output)
                        && !same_native_local(base_local, current_output)
                })
                && index_local.name == var
                && !index_local.deref
                && !index_local.is_persistent()
                && native_scalar_type(elem, element)
        }
        TExprKind::Binary { op, lhs, rhs, .. } => {
            matches!(op, BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div)
                && native_vector_expr_supported(lhs, var, element, extent, outputs, current_output)
                && native_vector_expr_supported(rhs, var, element, extent, outputs, current_output)
        }
        TExprKind::Unary {
            op: UnOp::Neg,
            operand,
        } => native_vector_expr_supported(operand, var, element, extent, outputs, current_output),
        _ => false,
    }
}

/// Build the lowering plan from a sema-attached loop fact. The shape checks
/// below select a renderer; they do not infer aliasing, purity, bounds, or
/// control-flow legality. Any unsupported shape returns to the ordinary TIR
/// emitter.
fn native_vectorization_plan(stmt: &TStmt) -> Option<NativeVectorPlan<'_>> {
    let TStmt::Range {
        label: None,
        var,
        source: None,
        start,
        end,
        step: None,
        exclusive: true,
        auto_vectorization: Some(facts),
        body,
    } = stmt
    else {
        return None;
    };
    if !is_complete_auto_vectorization(facts) || stmt.fact_channel().purity != TPurity::Pure {
        return None;
    }
    if !matches!(&start.kind, TExprKind::IntLit(0, _)) {
        return None;
    }
    let mut extent = match &end.kind {
        TExprKind::IntLit(value, _) if *value >= 0 => Some(usize::try_from(*value).ok()?),
        _ => None,
    };
    let element = NativeFloat::from_type(&facts.element_type)?;

    let mut stores: Vec<(&TLocal, &TExpr)> = Vec::with_capacity(body.len());
    let mut dynamic_roots = 0;
    for stmt in body {
        // Scoped lowering keeps source/debug metadata in the body. These nodes
        // have no runtime meaning and must not make a proven shape look like a
        // different loop; the original body remains available for the scalar
        // tail, where the ordinary emitter preserves its metadata.
        if matches!(stmt, TStmt::SourceSpan(_) | TStmt::LineMarker(_)) {
            continue;
        }
        let TStmt::IndexAssign {
            uninit: false,
            base,
            index,
            is_map: false,
            value,
        } = stmt
        else {
            return None;
        };
        let TExprKind::Local(base_local) = &base.kind else {
            return None;
        };
        let TExprKind::Local(index_local) = &index.kind else {
            return None;
        };
        let elem = match &base.ty {
            Type::FixedList { elem, len } => {
                if dynamic_roots != 0 {
                    return None;
                }
                let length = usize::try_from(len.literal_value()?).ok()?;
                if extent.is_some_and(|known| known != length) {
                    return None;
                }
                extent = Some(length);
                elem
            }
            Type::List(elem) => {
                if extent.is_some() || dynamic_roots != 0 {
                    return None;
                }
                dynamic_roots += 1;
                elem
            }
            _ => return None,
        };
        if !base_local.mutable && !base_local.deref
            || base_local.is_persistent()
            || index_local.name.as_str() != var.as_str()
            || index_local.deref
            || index_local.is_persistent()
            || !native_scalar_type(elem, element)
            || !native_scalar_type(&value.ty, element)
            || stores
                .iter()
                .any(|(output, _)| same_native_local(output, base_local))
        {
            return None;
        }
        stores.push((base_local, value));
    }
    if stores.is_empty() {
        return None;
    }
    let width = match extent {
        Some(extent) => element.width(extent)?,
        None => element.max_width(),
    };
    let outputs = stores.iter().map(|(local, _)| *local).collect::<Vec<_>>();
    if stores.iter().any(|(output, value)| {
        !native_vector_expr_supported(value, var, element, extent, &outputs, output)
    }) {
        return None;
    }
    Some(NativeVectorPlan {
        facts,
        element,
        width,
        extent,
        end,
        var,
        body,
        stores,
    })
}

fn native_simd_helper(element: NativeFloat, width: usize, op: BinOp) -> Option<&'static str> {
    match (element, width, op) {
        (NativeFloat::F32, 4, BinOp::Add) => Some("jet_simd_f32x4_add_array"),
        (NativeFloat::F32, 4, BinOp::Sub) => Some("jet_simd_f32x4_sub_array"),
        (NativeFloat::F32, 4, BinOp::Mul) => Some("jet_simd_f32x4_mul_array"),
        (NativeFloat::F32, 4, BinOp::Div) => Some("jet_simd_f32x4_div_array"),
        (NativeFloat::F32, 8, BinOp::Add) => Some("jet_simd_f32x8_add_array"),
        (NativeFloat::F32, 8, BinOp::Sub) => Some("jet_simd_f32x8_sub_array"),
        (NativeFloat::F32, 8, BinOp::Mul) => Some("jet_simd_f32x8_mul_array"),
        (NativeFloat::F32, 8, BinOp::Div) => Some("jet_simd_f32x8_div_array"),
        (NativeFloat::F64, 2, BinOp::Add) => Some("jet_simd_f64x2_add_array"),
        (NativeFloat::F64, 2, BinOp::Sub) => Some("jet_simd_f64x2_sub_array"),
        (NativeFloat::F64, 2, BinOp::Mul) => Some("jet_simd_f64x2_mul_array"),
        (NativeFloat::F64, 2, BinOp::Div) => Some("jet_simd_f64x2_div_array"),
        (NativeFloat::F64, 4, BinOp::Add) => Some("jet_simd_f64x4_add_array"),
        (NativeFloat::F64, 4, BinOp::Sub) => Some("jet_simd_f64x4_sub_array"),
        (NativeFloat::F64, 4, BinOp::Mul) => Some("jet_simd_f64x4_mul_array"),
        (NativeFloat::F64, 4, BinOp::Div) => Some("jet_simd_f64x4_div_array"),
        _ => None,
    }
}
/// A proven F64x4 loop can keep its intermediate values in the Prelude's
/// native carrier. The carrier is still a Prelude adapter: on targets without
/// AVX it aliases the same fixed-array implementation, so this changes only
/// register marshalling and never Jet's float operation semantics.
fn uses_native_f64x4_carrier(plan: &NativeVectorPlan<'_>) -> bool {
    matches!((plan.element, plan.width), (NativeFloat::F64, 4))
}


fn emit_native_vector_expr(
    expr: &TExpr,
    plan: &NativeVectorPlan<'_>,
    index: &str,
    cx: &Cx,
) -> String {
    let use_native = uses_native_f64x4_carrier(plan);
    match &expr.kind {
        TExprKind::IntLit(..) => {
            let value = emit_tir_expr(expr, cx);
            if use_native {
                format!(
                    "{}jet_simd_f64x4_splat_native(({}) as f64)",
                    cx.root_prefix, value
                )
            } else {
                format!(
                    "[({}) as {}; {}]",
                    value,
                    plan.element.scalar_name(),
                    plan.width
                )
            }
        }
        TExprKind::FloatLit(..) | TExprKind::Local(_) => {
            let value = emit_tir_expr(expr, cx);
            if use_native {
                format!("{}jet_simd_f64x4_splat_native({})", cx.root_prefix, value)
            } else {
                format!("[({}); {}]", value, plan.width)
            }
        }
        TExprKind::Index { base, .. } => {
            let TExprKind::Local(local) = &base.kind else {
                unreachable!("native vector expression plan validated the index base")
            };
            let lanes = (0..plan.width)
                .map(|lane| format!("({})[{} + {}]", local.rust_place(), index, lane))
                .collect::<Vec<_>>()
                .join(", ");
            if use_native {
                format!(
                    "{}jet_simd_f64x4_new_native([{lanes}])",
                    cx.root_prefix
                )
            } else {
                format!("[{lanes}]")
            }
        }
        TExprKind::Unary { operand, .. } => {
            let operand = emit_native_vector_expr(operand, plan, index, cx);
            if use_native {
                // The native carrier has no public negation adapter yet. Keep
                // this uncommon shape on the shared array helper, converting
                // only at the unary boundary.
                format!(
                    "{}jet_simd_f64x4_new_native({}jet_simd_f64x4_neg_array(&{}jet_simd_f64x4_to_array_native({operand})))",
                    cx.root_prefix, cx.root_prefix, cx.root_prefix
                )
            } else {
                let helper = match (plan.element, plan.width) {
                    (NativeFloat::F32, 4) => "jet_simd_f32x4_neg_array",
                    (NativeFloat::F32, 8) => "jet_simd_f32x8_neg_array",
                    (NativeFloat::F64, 2) => "jet_simd_f64x2_neg_array",
                    (NativeFloat::F64, 4) => "jet_simd_f64x4_neg_array",
                    _ => unreachable!("native vector negation width was validated"),
                };
                format!("{}{}(&({operand}))", cx.root_prefix, helper)
            }
        }
        TExprKind::Binary { op, lhs, rhs, .. } => {
            let lhs = emit_native_vector_expr(lhs, plan, index, cx);
            let rhs = emit_native_vector_expr(rhs, plan, index, cx);
            if use_native {
                let helper = match op {
                    BinOp::Add => "add",
                    BinOp::Sub => "sub",
                    BinOp::Mul => "mul",
                    BinOp::Div => "div",
                    _ => unreachable!("native vector binary operator was validated"),
                };
                format!(
                    "{}jet_simd_f64x4_{helper}_native(({lhs}), ({rhs}))",
                    cx.root_prefix
                )
            } else {
                let helper = native_simd_helper(plan.element, plan.width, *op)
                    .expect("native vector expression plan validated the binary operator");
                format!("{}{}(&({lhs}), &({rhs}))", cx.root_prefix, helper)
            }
        }
        _ => unreachable!("native vector expression plan validated the expression shape"),
    }
}


fn emit_native_vectorized_range(
    plan: &NativeVectorPlan<'_>,
    cx: &Cx,
    out: &mut String,
    indent: usize,
) {
    let pad = "    ".repeat(indent);
    let index = mangle_generated("simd_index");
    let end = mangle_generated("simd_end");
    let extent = plan
        .extent
        .map_or_else(|| "dynamic".to_string(), |extent| extent.to_string());
    out.push_str(&format!(
        "{pad}/* jet-auto-vectorize-loop: backend=prelude-runtime-dispatch element={} width={} extent={} */\n",
        plan.facts.element_type.name(), plan.width, extent
    ));
    out.push_str(&format!("{pad}let mut {index}: usize = 0;\n"));
    out.push_str(&format!(
        "{pad}let {end}: usize = ({}) as usize;\n",
        emit_tir_expr(plan.end, cx)
    ));
    out.push_str(&format!(
        "{pad}while {index} + {} <= {end} {{\n",
        plan.width
    ));
    let use_native = uses_native_f64x4_carrier(plan);
    let vector_values = plan
        .stores
        .iter()
        .enumerate()
        .map(|(slot, (_, value))| {
            let name = mangle_generated(&format!("simd_value_{slot}"));
            let value = emit_native_vector_expr(value, plan, &index, cx);
            out.push_str(&format!(
                "{}let {name} = {value};\n",
                "    ".repeat(indent + 1)
            ));
            if use_native {
                let array_name = mangle_generated(&format!("simd_array_{slot}"));
                out.push_str(&format!(
                    "{}let {array_name} = {}jet_simd_f64x4_to_array_native({name});\n",
                    "    ".repeat(indent + 1),
                    cx.root_prefix,
                ));
                array_name
            } else {
                name
            }
        })
        .collect::<Vec<_>>();
    for (slot, (local, _)) in plan.stores.iter().enumerate() {
        for lane in 0..plan.width {
            out.push_str(&format!(
                "{}{}[{} + {}] = {}[{}];\n",
                "    ".repeat(indent + 1),
                local.rust_place(),
                index,
                lane,
                vector_values[slot],
                lane
            ));
        }
    }
    out.push_str(&format!(
        "{}{} += {};\n",
        "    ".repeat(indent + 1),
        index,
        plan.width
    ));
    out.push_str(&format!("{pad}}}\n"));
    if plan.extent.map_or(true, |extent| extent % plan.width != 0) {
        let loop_var = mangle(plan.var);
        out.push_str(&format!(
            "{pad}for {loop_var} in ({index} as i64)..({end} as i64) {{\n"
        ));
        emit_tir_stmts(plan.body, cx, out, indent + 1);
        out.push_str(&format!("{pad}}}\n"));
    }
}

/// A vectorized body is emitted in three independent lexical slices. That is
/// safe only when no resource/deferred-close state crosses the candidate;
/// otherwise the ordinary emitter retains its cleanup stack and semantics.
fn contains_native_cleanup_boundary(stmts: &[TStmt]) -> bool {
    stmts.iter().any(|stmt| match stmt {
        TStmt::DeferClose { .. } => true,
        TStmt::Let { init, .. } => matches!(&init.kind, TExprKind::ResourceNew(_)),
        TStmt::RefutableBind { fallback, .. } => contains_native_cleanup_boundary(fallback),
        TStmt::GcEdit { stmt, .. } => {
            contains_native_cleanup_boundary(std::slice::from_ref(stmt.as_ref()))
        }
        TStmt::ContractScope { body, .. }
        | TStmt::TaskGroup { body, .. }
        | TStmt::Loop { body, .. }
        | TStmt::While { body, .. }
        | TStmt::Range { body, .. }
        | TStmt::ForIn { body, .. }
        | TStmt::Inline(body)
        | TStmt::DebugOnly(body)
        | TStmt::Unsafe { body, .. }
        | TStmt::SentryPolicy { body, .. }
        | TStmt::Impure(body)
        | TStmt::Region(body)
        | TStmt::Layout { body, .. }
        | TStmt::ContextBlock { body, .. }
        | TStmt::Live { body }
        | TStmt::Shield { body }
        | TStmt::ScopeMember { body, .. }
        | TStmt::Transact { body, .. } => contains_native_cleanup_boundary(body),
        TStmt::CountedLoop {
            init, step, body, ..
        } => {
            contains_native_cleanup_boundary(std::slice::from_ref(init.as_ref()))
                || step.as_deref().is_some_and(|step| {
                    contains_native_cleanup_boundary(std::slice::from_ref(step))
                })
                || contains_native_cleanup_boundary(body)
        }
        TStmt::If {
            then_body,
            else_body,
            ..
        } => {
            contains_native_cleanup_boundary(then_body)
                || else_body
                    .as_deref()
                    .is_some_and(contains_native_cleanup_boundary)
        }
        TStmt::EnumMatch {
            arms, else_body, ..
        } => {
            arms.iter()
                .any(|arm| contains_native_cleanup_boundary(&arm.body))
                || else_body
                    .as_deref()
                    .is_some_and(contains_native_cleanup_boundary)
        }
        TStmt::RangeSwitch {
            arms, else_body, ..
        } => {
            arms.iter()
                .any(|(_, _, body)| contains_native_cleanup_boundary(body))
                || contains_native_cleanup_boundary(else_body)
        }
        TStmt::MixedSwitch {
            arms, else_body, ..
        } => {
            arms.iter()
                .any(|(_, body)| contains_native_cleanup_boundary(body))
                || else_body
                    .as_deref()
                    .is_some_and(contains_native_cleanup_boundary)
        }
        _ => false,
    })
}

fn native_vectorization_candidate(stmts: &[TStmt]) -> Option<(usize, NativeVectorPlan<'_>)> {
    if contains_native_cleanup_boundary(stmts) {
        return None;
    }
    stmts.iter().enumerate().find_map(|(index, stmt)| {
        let plan = native_vectorization_plan(stmt)?;
        Some((index, plan))
    })
}

fn emit_tir_stmts_with_native_vectorization(
    stmts: &[TStmt],
    cx: &Cx,
    out: &mut String,
    indent: usize,
) -> bool {
    if cx.scalar_function.get() {
        return false;
    }
    if contains_native_cleanup_boundary(stmts) {
        return false;
    }
    let mut emitted = false;
    let mut ordinary_start = 0;
    for index in 0..stmts.len() {
        let Some(plan) = native_vectorization_plan(&stmts[index]) else {
            continue;
        };
        emit_tir_stmts(&stmts[ordinary_start..index], cx, out, indent);
        emit_native_vectorized_range(&plan, cx, out, indent);
        ordinary_start = index + 1;
        emitted = true;
    }
    if !emitted {
        return false;
    }
    emit_tir_stmts(&stmts[ordinary_start..], cx, out, indent);
    true
}

/// D-SIMD3=B: render the actual native vector boundary only when sema supplied
/// the complete proof and this emitter has a matching fixed-array lowering.
/// #Scalar suppresses it; its loop barrier is emitted instead.
fn auto_vectorization_attr(tir: &TFunc, indent: usize) -> String {
    let is_wrapped_body = tir.is_reactive
        || matches!(&tir.ret, Some(Type::Apply { name, .. }) if name == crate::Syntax::TYPE_STREAM);
    let Some(facts) =
        first_auto_vectorization(&tir.body).filter(|_| !tir.is_scalar && !is_wrapped_body)
    else {
        return String::new();
    };
    let pad = "    ".repeat(indent);
    let inline = if tir.is_inline || tir.is_inline_always {
        ""
    } else {
        "#[inline(always)]\n"
    };
    format!(
        "{pad}/* jet-auto-vectorize: backend=prelude-runtime-dispatch element={} no_aliasing={} no_early_exit={} effect_free_body={} no_cross_iteration_deps={} */\n\
{pad}{inline}",
        facts.element_type.name(),
        facts.no_aliasing,
        facts.no_early_exit,
        facts.effect_free_body,
        facts.no_cross_iteration_deps,
    )
}

/// Read only the fact sema attached to a TIR loop. This candidate check never
/// reconstructs legality from lowered expressions; it only finds the first
/// proof so the enclosing native function gets the same codegen hint.
fn first_auto_vectorization(
    stmts: &[crate::Codegen::TIR::TStmt],
) -> Option<crate::AST::AutoVectorizationFacts> {
    native_vectorization_candidate(stmts).map(|(_, plan)| plan.facts.clone())
}

/// A vectorization hint is valid only for the complete sema proof. The
/// channel's purity and dependency bits are useful cross-consumer facts, but
/// they do not replace the aliasing and control-flow obligations carried by
/// this record.
fn is_complete_auto_vectorization(facts: &crate::AST::AutoVectorizationFacts) -> bool {
    facts.no_aliasing
        && facts.no_early_exit
        && facts.effect_free_body
        && facts.no_cross_iteration_deps
}

/// D-CMD-OVERRIDE1=C: `TestSuite` is a `Copy` snapshot, and the
/// ratified override signature binds one by value — `fn test(suite: TestSuite)`.
/// Their one method, `run`, mutates the receiver, which the shared-reference
/// form of a non-scalar `Read` parameter cannot express: `rust_param_type`
/// renders the parameter `&T`, so `jet_test_suite_run(&mut (*__jet_suite))`
/// borrows through a shared reference and rustc rejects the emitted program —
/// an I2 compiler bug, never the user's fault.
/// A source `struct TestSuite` shadows the core name (the same
/// `!cx.type_names.contains(..)` guard `Cx::rust_type` applies), and that value
/// is an ordinary user struct with ordinary `Read` semantics.
fn is_owned_snapshot_param(cx: &Cx, convention: AccessConvention, ty: &Type) -> bool {
    let Type::Named(name) = ty else { return false };
    matches!(convention, AccessConvention::Read)
        && name == "TestSuite"
        && !cx.type_names.contains(name.as_str())
}

/// Re-bind every command-suite parameter to the callee's own mutable copy at
/// the head of the body, so a `suite.run()` that mutates is expressible. Both
/// other tiers already hand the override body a suite it mutates in place (the
/// TIR evaluator writes back into the receiver `CtValue`, the JIT writes back
/// into the suite record), so `suite.iteration`/`suite.result` read the same
/// post-`run` values under the default `jet run`, the resident JIT, and AOT
/// (I9). Emits nothing for a function that takes no suite.
fn emit_owned_snapshot_params(tir: &TFunc, cx: &Cx, out: &mut String, indent: usize) {
    let pad = "    ".repeat(indent);
    for (rust_name, ty, convention) in &tir.params {
        if !is_owned_snapshot_param(cx, *convention, ty) {
            continue;
        }
        out.push_str(&format!("{pad}let mut {rust_name} = *{rust_name};\n"));
        out.push_str(&format!("{pad}let {rust_name} = &mut {rust_name};\n"));
    }
}

/// Emit a covered function from its TIR, reusing the same pure formatting helpers
/// as `emit_func` so the output is byte-identical to the AST path (golden parity).
/// The only difference is that every decision is *read off the TIR* rather than
/// recomputed — there is no `expr_jet_ty` / `operand_is_integer` call anywhere.

pub(crate) fn emit_tir_func(tir: &TFunc, cx: &Cx, out: &mut String) {
    cx.time_emission(|| emit_tir_func_inner(tir, cx, out));
}

fn emit_tir_func_inner(tir: &TFunc, cx: &Cx, out: &mut String) {
    match &tir.kind {
        TFuncKind::TopLevel => emit_tir_toplevel(tir, cx, out),
        TFuncKind::Method { self_conv, .. } => emit_tir_method(tir, *self_conv, cx, out),
        TFuncKind::TraitMethod {
            is_unsafe,
            self_conv,
            serde,
        } => emit_tir_trait_method(tir, *is_unsafe, *self_conv, *serde, cx, out),
        TFuncKind::Delegation {
            sig,
            fwd,
            has_return,
        } => emit_tir_delegation(tir, sig, fwd, *has_return, cx, out),
    }
}

/// A module-level free function: `pub fn name(params) -> ret { … }`.
/// Byte-identical to `emit_func`'s output.
pub(crate) fn emit_tir_toplevel(tir: &TFunc, cx: &Cx, out: &mut String) {
    let view_provenance = tir.return_view_provenance.as_ref();
    let view_owner_params = view_provenance
        .into_iter()
        .flat_map(|map| map.values())
        .flat_map(|provenance| provenance.sources.iter())
        .filter_map(|source| match source.source {
            ViewSource::Parameter(index) => Some(index),
            _ => None,
        })
        .collect::<std::collections::BTreeSet<_>>();
    let has_view_return = view_provenance.is_some_and(|map| !map.is_empty());
    let ret_clause = match &tir.ret {
        Some(t) => {
            let rust = if has_view_return {
                cx.rust_type_with_view_lifetime(t)
            } else {
                rust_return_type(cx, t)
            };
            let rust = if tir.gc_return {
                format!("{}jet_gc::AutomaticRoot<{rust}>", cx.root_prefix)
            } else {
                rust
            };
            format!(" -> {rust}")
        }
        None => String::new(),
    };
    let params = tir
        .params
        .iter()
        .enumerate()
        .map(|(index, (rust_name, ty, conv))| {
            let rust = rust_param_type(cx, *conv, ty);
            let rust = if view_owner_params.contains(&index) {
                add_hidden_view_lifetime(rust)
            } else {
                rust
            };
            format!("{rust_name}: {rust}")
        })
        .collect::<Vec<_>>()
        .join(", ");
    let vis = if tir.is_main { "" } else { "pub " };
    // c109 Phase 18: an `#Unsafe fn` lowers to `unsafe fn` — the prefix sits right after
    // `vis`, exactly as `emit_func` (`{vis}{unsafe_kw}fn …`). I1: emitted ONLY when the
    // source was `#Unsafe fn` (`tir.is_unsafe`).
    let unsafe_kw = if tir.is_unsafe { "unsafe " } else { "" };
    // D-CABI-CALLBACK1: `extern "C" fn` ONLY for a function sema proved is
    // actually passed as a native callback symbol somewhere (`cx.ffi_callback_fns`,
    // built from `CallArgFlags::c_callback_symbol` — see
    // `crates/jet-sema/src/Sema/Bundle.rs::collect_core_expr`). Never every
    // `#Pure fn`: that leaked the purity lever into codegen and broke I3
    // erasure (`effect_annotations_are_erased`, `eff2_levers_are_erased`,
    // fixed by 14dd68a5) — but a bare fn reference handed to a `#Extern`
    // C-ABI callback parameter (`callback_twice(increment, x)`) genuinely
    // needs the C calling convention: the referenced Rust item's own type
    // must match the raw `extern "C" fn` pointer type the C side expects.
    let ffi_callback = cx.ffi_callback_fns.contains(&tir.name) && tir.generics.is_empty();
    let abi = if ffi_callback { "extern \"C\" " } else { "" };
    // D-METHODMACRO1=A: `#Inline`/`#Inline(Always)` lower to a Rust `#[inline]`/
    // `#[inline(always)]` attribute right above the signature. `is_inline_always`
    // is only ever `true` here once sema has confirmed the function can actually
    // inline (E0917/E0918/E0919 would have failed the build otherwise) — I3:
    // sema decides, codegen just emits.
    let inline_attr = if tir.is_scalar {
        "#[inline(never)]\n"
    } else if tir.is_inline_always {
        "#[inline(always)]\n"
    } else if tir.is_inline {
        "#[inline]\n"
    } else {
        ""
    };
    let auto_vectorization = auto_vectorization_attr(tir, 0);
    let kernel_proof = tir
        .kernel_proof
        .map(|proof| {
            format!(
                "const _: () = assert!({}, \"Jet kernel proof must be complete\");\n\
/* jet-kernel-proof: mode={} bounds={} alias_free={} captures={} race_free={} barriers_uniform={} control_flow={} */\n",
                proof.is_complete(),
                proof.mode.as_str(),
                proof.bounds,
                proof.alias_free,
                proof.captures,
                proof.race_free,
                proof.barriers_uniform,
                proof.control_flow,
            )
        })
        .unwrap_or_default();
    // E2-M12 D-OBS1: track the current function name for rich panic reports —
    // matches `emit_func` so panic output is identical.
    *cx.current_fn.borrow_mut() = tir.name.clone();
    cx.current_fn_line
        .set(u32::try_from(tir.line).unwrap_or(u32::MAX));
    let generics = if has_view_return {
        add_hidden_view_generic(&tir.generics)
    } else {
        tir.generics.clone()
    };
    // D-DATARACE1=C: surface synchronized-form upgrades before the Rust fn.
    for line in &tir.reactive_upgrades {
        out.push_str(&format!("/* jet-reactive-upgrade: {line} */\n"));
    }
    if let Some(bound) = tir.memo_bound {
        emit_tir_memoized_toplevel(
            tir,
            cx,
            out,
            &params,
            &ret_clause,
            &generics,
            vis,
            unsafe_kw,
            abi,
            inline_attr,
            kernel_proof,
            bound,
        );
        return;
    }
    out.push_str(&format!(
        "{auto_vectorization}{kernel_proof}{inline_attr}{vis}{unsafe_kw}{abi}fn {name}{gen}({params}){ret} {{\n",
        name = cx.mangle_name(&tir.name),
        gen = generics,
        params = params,
        ret = ret_clause,
        abi = abi,
    ));
    if ffi_callback {
        // D-FFI-UNIFY1 / card #1121: no Rust unwind may cross a foreign
        // callback frame. Prelude owns failure conversion; this emitter only
        // supplies the callback body and ABI spelling.
        out.push_str(&format!(
            "    {}jet_ffi_callback_boundary(|| {{\n",
            cx.root_prefix
        ));
        emit_tir_function_body(tir, cx, out, 2);
        out.push_str("    })\n");
    } else {
        emit_tir_function_body(tir, cx, out, 1);
    }
    out.push_str("}\n\n");
}

/// D-MEMO1=A: emit one public function wrapper around one private body and one
/// per-function Prelude store. The wrapper owns only argument/result marshalling;
/// bound handling, LRU order, and counters remain in `Prelude/Memo.rs`.
fn emit_tir_memoized_toplevel(
    tir: &TFunc,
    cx: &Cx,
    out: &mut String,
    params: &str,
    ret_clause: &str,
    generics: &str,
    vis: &str,
    unsafe_kw: &str,
    abi: &str,
    inline_attr: &str,
    kernel_proof: String,
    bound: Option<usize>,
) {
    let name = cx.mangle_name(&tir.name);
    let store_name = jet_name_format!("{name_prefix}memo_store_{name}");
    let body_name = jet_name_format!("{name_prefix}memo_body_{name}");
    let stats_name = jet_name_format!("{name_prefix}memo_stats_{name}");
    let key_type = memo_key_type(tir, cx);
    let value_type = ret_clause.strip_prefix(" -> ").unwrap_or("()").to_string();
    let key_expr = memo_key_expr(tir, cx);
    let call_args = tir
        .params
        .iter()
        .map(|(rust_name, _, _)| rust_name.as_str())
        .collect::<Vec<_>>()
        .join(", ");
    let body_call = if tir.is_unsafe {
        format!("unsafe {{ {body_name}({call_args}) }}")
    } else {
        format!("{body_name}({call_args})")
    };
    let bound_expr = bound
        .map(|bound| format!("Some({bound})"))
        .unwrap_or_else(|| "None".to_string());
    let body_unsafe = if tir.is_unsafe { "unsafe " } else { "" };
    let root = &cx.root_prefix;
    let store_init = format!(
        "{store_name}.get_or_init(|| ::std::sync::Mutex::new({root}JetMemo::with_bound({bound_expr})))"
    );

    out.push_str(&format!(
        "static {store_name}: ::std::sync::OnceLock<::std::sync::Mutex<{root}JetMemo<{key_type}, {value_type}>>> = ::std::sync::OnceLock::new();\n\n"
    ));
    out.push_str(&format!(
        "{body_unsafe}fn {body_name}{generics}({params}){ret_clause} {{\n"
    ));
    emit_tir_function_body(tir, cx, out, 1);
    out.push_str("}\n\n");
    out.push_str(&format!(
        "{kernel_proof}{inline_attr}{vis}{unsafe_kw}{abi}fn {name}{generics}({params}){ret_clause} {{\n"
    ));
    out.push_str(&jet_name_format!(
        "    let {name_prefix}memo_store = {store_init};\n    let {name_prefix}memo_key: {key_type} = {key_expr};\n    {root}jet_memo_call({name_prefix}memo_store, {name_prefix}memo_key, || {body_call})\n"
    ));
    out.push_str("}\n\n");
    out.push_str(&format!(
        "pub fn {stats_name}() -> {root}JetMemoStats {{\n    {store_init}.lock().unwrap_or_else(|error| error.into_inner()).stats()\n}}\n\n"
    ));
}

fn memo_key_type(tir: &TFunc, cx: &Cx) -> String {
    let types = tir
        .params
        .iter()
        .map(|(_, ty, _)| cx.rust_type(ty))
        .collect::<Vec<_>>();
    match types.as_slice() {
        [] => "()".to_string(),
        [ty] => format!("({ty},)"),
        _ => format!("({})", types.join(", ")),
    }
}

fn memo_key_expr(tir: &TFunc, cx: &Cx) -> String {
    let values = tir
        .params
        .iter()
        .map(|(rust_name, ty, convention)| {
            let parameter_type = rust_param_type(cx, *convention, ty);
            if parameter_type.starts_with('&') {
                format!("(*{rust_name}).clone()")
            } else {
                format!("{rust_name}.clone()")
            }
        })
        .collect::<Vec<_>>();
    match values.as_slice() {
        [] => "()".to_string(),
        [value] => format!("({value},)"),
        _ => format!("({})", values.join(", ")),
    }
}

fn emit_tir_function_body(tir: &TFunc, cx: &Cx, out: &mut String, indent: usize) {
    let previous_scalar = cx.scalar_function.replace(tir.is_scalar);
    emit_stack_guard(tir, cx, out, indent);
    emit_sentry_gate(tir, cx, out, indent);
    emit_sentry_frame(tir, cx, out, indent);
    emit_owned_snapshot_params(tir, cx, out, indent);
    // D-COV1: probe at the function head (skip the synthetic `main`).
    if cx.coverage && !tir.is_main {
        out.push_str(&format!(
            "{}{}jet_cov({});\n",
            "    ".repeat(indent),
            cx.root_prefix,
            tir.line
        ));
    }
    if tir.is_reactive {
        emit_reactive_wrapped_body(&tir.body, cx, out, indent);
    } else if matches!(&tir.ret, Some(Type::Apply { name, .. }) if name == crate::Syntax::TYPE_STREAM)
    {
        emit_generator_wrapped_body(&tir.body, cx, out, indent);
    } else {
        if !emit_tir_stmts_with_native_vectorization(&tir.body, cx, out, indent) {
            emit_tir_stmts(&tir.body, cx, out, indent);
        }
    }
    if is_fallible_void_return(&tir.ret) {
        out.push_str(&format!("{}Ok(())\n", "    ".repeat(indent)));
    }
    cx.scalar_function.set(previous_scalar);
}

fn add_hidden_view_lifetime(rust_type: String) -> String {
    if let Some(rest) = rust_type.strip_prefix("&mut ") {
        jet_format!("&'{jet_prefix}view mut {rest}")
    } else if let Some(rest) = rust_type.strip_prefix('&') {
        jet_format!("&'{jet_prefix}view {rest}")
    } else {
        rust_type
    }
}

fn add_hidden_view_generic(generics: &str) -> String {
    if generics.is_empty() {
        jet_format!("<'{jet_prefix}view>")
    } else if let Some(rest) = generics.strip_prefix('<') {
        jet_format!("<'{jet_prefix}view, {rest}")
    } else {
        generics.to_string()
    }
}

fn is_fallible_void_return(ret: &Option<Type>) -> bool {
    matches!(
        ret,
        Some(Type::Result { ok, .. })
            if matches!(ok.as_ref(), Type::Named(n) if n == crate::Syntax::INTERNAL_UNIT_TYPE)
    )
}

/// D-CONC-STREAM1=A / D-CANCELMODEL1=C: a generator (`=> Stream<T>`) is a
/// scheduler child. The shared Stream Prelude owns its pull protocol and the
/// task cancellation handle; `yield` is the producer's wait point.
fn emit_generator_wrapped_body(body: &[TStmt], cx: &Cx, out: &mut String, indent: usize) {
    let pad = "    ".repeat(indent);
    let inner = indent + 1;
    out.push_str(&jet_format!(
        "{pad}{root_prefix}jet_std::jet_stream_task(move |{jet_prefix}yield_tx| {{\n",
        root_prefix = cx.root_prefix
    ));
    emit_tir_stmts(body, cx, out, inner);
    out.push_str(&format!("{}}})\n", pad));
}

fn emit_reactive_wrapped_body(body: &[TStmt], cx: &Cx, out: &mut String, indent: usize) {
    let pad = "    ".repeat(indent);
    let inner = indent + 1;
    out.push_str(&format!(
        "{}{}jet_std::jet_reactive_effect_rooted({});\n",
        pad,
        cx.root_prefix,
        render_reactive_tir_closure(body, cx, inner)
    ));
}

fn render_reactive_tir_closure(body: &[TStmt], cx: &Cx, indent: usize) -> String {
    let mut inner = String::new();
    emit_tir_stmts(body, cx, &mut inner, indent);
    format!("move || {{ {} }}", inner)
}

/// c109 Phase 7: an inherent method, emitted INSIDE an `impl __jet_<T> { … }` block
/// (the caller `emit_type_impl` already opened it). Byte-identical to `emit_method`:
/// `    pub fn __jet_<name>(<self>, <params>) -> <ret> {\n … \n    }\n`. The `self`
/// receiver form comes from `self_conv` (`Read`→`&self`, `Mutate`→`&mut self`,
/// `Move`→`self`); a static method (`self_conv == None`) emits no receiver.
pub(crate) fn emit_tir_method(
    tir: &TFunc,
    self_conv: Option<AccessConvention>,
    cx: &Cx,
    out: &mut String,
) {
    let indent = 1;
    let pad = "    ".repeat(indent);
    let view_provenance = tir.return_view_provenance.as_ref();
    let has_view_return = view_provenance.is_some_and(|map| !map.is_empty());
    let borrows_receiver = view_provenance.is_some_and(|map| {
        map.values().any(|provenance| {
            provenance
                .sources
                .iter()
                .any(|source| matches!(source.source, ViewSource::Receiver))
        })
    });
    let ret_clause = match &tir.ret {
        Some(t) => {
            let rust = if has_view_return {
                cx.rust_type_with_view_lifetime(t)
            } else {
                rust_return_type(cx, t)
            };
            let rust = if tir.gc_return {
                format!("{}jet_gc::AutomaticRoot<{rust}>", cx.root_prefix)
            } else {
                rust
            };
            format!(" -> {rust}")
        }
        None => String::new(),
    };
    let mut params: Vec<String> = Vec::new();
    if let Some(conv) = self_conv {
        params.push(
            match conv {
                AccessConvention::Read if borrows_receiver => {
                    jet_format!("&'{jet_prefix}view self")
                }
                AccessConvention::Write if borrows_receiver => {
                    jet_format!("&'{jet_prefix}view mut self")
                }
                AccessConvention::Read => "&self".to_string(),
                AccessConvention::Write => "&mut self".to_string(),
                AccessConvention::Move => "self".to_string(),
            }
            .to_string(),
        );
    }
    for (index, (rust_name, ty, conv)) in tir.params.iter().enumerate() {
        let rust = rust_param_type(cx, *conv, ty);
        let rust = if view_provenance.is_some_and(|map| {
            map.values().any(|provenance| {
                provenance.sources.iter().any(
                |source| matches!(source.source, ViewSource::Parameter(owner) if owner == index),
            )
            })
        }) {
            add_hidden_view_lifetime(rust)
        } else {
            rust
        };
        params.push(format!("{rust_name}: {rust}"));
    }
    // c109 Phase 18: an `#Unsafe fn` inherent method lowers to `pub unsafe fn` — the
    // prefix sits between `pub ` and `fn`, exactly as `emit_method` (`pub {unsafe_kw}fn`).
    // I1: emitted ONLY for a source `#Unsafe fn` (`tir.is_unsafe`).
    let unsafe_kw = if tir.is_unsafe { "unsafe " } else { "" };
    // D-METHODMACRO1=A: `#Inline`/`#Inline(Always)` on a method — same attribute,
    // indented to the method's own line (see `emit_tir_toplevel` for the free-
    // function form).
    let inline_attr = if tir.is_scalar {
        format!("{pad}#[inline(never)]\n")
    } else if tir.is_inline_always {
        format!("{pad}#[inline(always)]\n")
    } else if tir.is_inline {
        format!("{pad}#[inline]\n")
    } else {
        String::new()
    };
    let auto_vectorization = auto_vectorization_attr(tir, indent);
    let previous_scalar = cx.scalar_function.replace(tir.is_scalar);
    // E2-M12 D-OBS1: track the current function name for rich panic reports.
    *cx.current_fn.borrow_mut() = tir.name.clone();
    cx.current_fn_line
        .set(u32::try_from(tir.line).unwrap_or(u32::MAX));
    for line in &tir.reactive_upgrades {
        out.push_str(&format!("{pad}/* jet-reactive-upgrade: {line} */\n"));
    }
    let (method_generics, method_where) = tir
        .generics
        .split_once(" where ")
        .map_or((tir.generics.as_str(), ""), |(generics, bounds)| {
            (generics, bounds)
        });
    let method_generics = if has_view_return {
        if method_generics.is_empty() {
            jet_format!("<'{jet_prefix}view>")
        } else {
            jet_format!("<'{jet_prefix}view, {}", &method_generics[1..])
        }
    } else {
        method_generics.to_string()
    };
    let method_where = if method_where.is_empty() {
        String::new()
    } else {
        format!(" where {method_where}")
    };
    out.push_str(&format!(
        "{auto_vectorization}{inline_attr}{pad}pub {unsafe_kw}fn {name}{method_generics}({params}){ret}{method_where} {{\n",
        name = mangle(&tir.name),
        params = params.join(", "),
        ret = ret_clause,
    ));
    emit_stack_guard(tir, cx, out, indent + 1);
    emit_sentry_gate(tir, cx, out, indent + 1);
    emit_sentry_frame(tir, cx, out, indent + 1);
    emit_owned_snapshot_params(tir, cx, out, indent + 1);
    // D-COV1: probe at the method head.
    if cx.coverage {
        out.push_str(&format!(
            "{pad}    {}jet_cov({});\n",
            cx.root_prefix, tir.line
        ));
    }
    if tir.is_reactive {
        emit_reactive_wrapped_body(&tir.body, cx, out, indent + 1);
    } else if let Some(field) = &tir.memo_field {
        let memo_value = {
            let mut value = None;
            for stmt in &tir.body {
                match stmt {
                    TStmt::LineMarker(_) | TStmt::SourceSpan(_) => {}
                    TStmt::Return(Some(expr)) if value.is_none() => value = Some(expr),
                    _ => {
                        value = None;
                        break;
                    }
                }
            }
            value
        };
        if let Some(value) = memo_value {
            let storage = crate::Syntax::memo_storage_name(field);
            let value = emit_tir_expr(value, cx);
            out.push_str(&format!(
                "{pad}    (self).{storage}.get_or_insert_with(|| {{ {value} }})\n"
            ));
        } else {
            if !emit_tir_stmts_with_native_vectorization(&tir.body, cx, out, indent + 1) {
                emit_tir_stmts(&tir.body, cx, out, indent + 1);
            }
        }
    } else {
        if !emit_tir_stmts_with_native_vectorization(&tir.body, cx, out, indent + 1) {
            emit_tir_stmts(&tir.body, cx, out, indent + 1);
        }
    }
    if is_fallible_void_return(&tir.ret) {
        out.push_str(&format!("{pad}    Ok(())\n"));
    }
    cx.scalar_function.set(previous_scalar);
    out.push_str(&format!("{pad}}}\n"));
}

/// c109 Phase 12: a trait-impl method, emitted INSIDE an `impl Trait for __jet_<T> { … }`
/// block (the caller `emit_trait_impl`/`emit_external_trait_impl` opened it).
/// Byte-identical to `emit_trait_method` (Source/Codegen/Items.rs): a BARE method name
/// (no `__jet_` mangle — the trait owns it), NO `pub`, an always-`&self` receiver, and
/// an `unsafe ` prefix iff the source was an `#Unsafe fn`.
pub(crate) fn emit_tir_trait_method(
    tir: &TFunc,
    is_unsafe: bool,
    self_conv: Option<AccessConvention>,
    serde: Option<SerdeCodec>,
    cx: &Cx,
    out: &mut String,
) {
    // D-SERDE2 (card #131 S1-bridge): a hand `impl T.Encode`/`impl T.Decode` method is
    // bridged to the Rust `__jet_Encode`/`__jet_Decode` trait's method name + signature.
    // The user wrote the verbs `encode`/`decode` with Jet-facing signatures; the trait
    // declares `jet_encode(&self) -> jet_std::DataTree` /
    // `jet_decode(tree: &jet_std::DataTree) -> Result<Self, Vec<jet_std::FieldError>>`.
    if let Some(codec) = serde {
        emit_tir_serde_method(tir, codec, cx, out);
        return;
    }
    let indent = 1;
    let pad = "    ".repeat(indent);
    let view_provenance = tir.return_view_provenance.as_ref();
    let has_view_return = view_provenance.is_some_and(|map| !map.is_empty());
    let borrows_receiver = view_provenance.is_some_and(|map| {
        map.values().any(|provenance| {
            provenance
                .sources
                .iter()
                .any(|source| matches!(source.source, ViewSource::Receiver))
        })
    });
    let ret_clause = match &tir.ret {
        // `emit_trait_method` computes `ret = rust_return_type(...)` then, if non-empty,
        // ` -> ret`. A unit return yields the empty clause.
        Some(t) => {
            let ret = if has_view_return {
                cx.rust_type_with_view_lifetime(t)
            } else {
                rust_return_type(cx, t)
            };
            let ret = if tir.gc_return {
                format!("{}jet_gc::AutomaticRoot<{ret}>", cx.root_prefix)
            } else {
                ret
            };
            if ret.is_empty() {
                String::new()
            } else {
                format!(" -> {}", ret)
            }
        }
        None => String::new(),
    };
    // D-MUTSELF1: the receiver honors the source convention — `&self` / `&mut self` /
    // `self` — matching `emit_trait_method` and the trait declaration (emit_trait_def).
    let mut params: Vec<String> = self_conv
        .map(|conv| match conv {
            AccessConvention::Read if borrows_receiver => {
                jet_format!("&'{jet_prefix}view self")
            }
            AccessConvention::Write if borrows_receiver => {
                jet_format!("&'{jet_prefix}view mut self")
            }
            AccessConvention::Read => "&self".to_string(),
            AccessConvention::Write => "&mut self".to_string(),
            AccessConvention::Move => "self".to_string(),
        })
        .into_iter()
        .collect();
    for (index, (rust_name, ty, conv)) in tir.params.iter().enumerate() {
        let rust = rust_param_type(cx, *conv, ty);
        let rust = if view_provenance.is_some_and(|map| {
            map.values().any(|provenance| {
                provenance.sources.iter().any(
                |source| matches!(source.source, ViewSource::Parameter(owner) if owner == index),
            )
            })
        }) {
            add_hidden_view_lifetime(rust)
        } else {
            rust
        };
        params.push(format!("{rust_name}: {rust}"));
    }
    let unsafe_kw = if is_unsafe { "unsafe " } else { "" };
    let scalar_attr = if tir.is_scalar {
        format!("{pad}#[inline(never)]\n")
    } else {
        String::new()
    };
    let auto_vectorization = auto_vectorization_attr(tir, indent);
    // E2-M12 D-OBS1: track the current function name for rich panic reports.
    *cx.current_fn.borrow_mut() = tir.name.clone();
    cx.current_fn_line
        .set(u32::try_from(tir.line).unwrap_or(u32::MAX));
    out.push_str(&format!(
        "{auto_vectorization}{scalar_attr}{pad}{unsafe_kw}fn {name}{generics}{view_generic}({params}){ret} {{\n",
        name = tir.name,
        generics = tir.generics,
        view_generic = if has_view_return {
            jet_format!("<'{jet_prefix}view>")
        } else {
            String::new()
        },
        params = params.join(", "),
        ret = ret_clause,
    ));
    emit_stack_guard(tir, cx, out, indent + 1);
    emit_sentry_gate(tir, cx, out, indent + 1);
    emit_sentry_frame(tir, cx, out, indent + 1);
    emit_owned_snapshot_params(tir, cx, out, indent + 1);
    // D-COV1: probe at the trait-method head.
    if cx.coverage {
        out.push_str(&format!(
            "{pad}    {}jet_cov({});\n",
            cx.root_prefix, tir.line
        ));
    }
    let previous_scalar = cx.scalar_function.replace(tir.is_scalar);
    if !emit_tir_stmts_with_native_vectorization(&tir.body, cx, out, indent + 1) {
        emit_tir_stmts(&tir.body, cx, out, indent + 1);
    }
    cx.scalar_function.set(previous_scalar);
    out.push_str(&format!("{pad}}}\n"));
}

/// D-SERDE2 (card #131 S1-bridge): emit a hand `impl T.Encode`/`impl T.Decode` method,
/// bridged to the Rust `__jet_Encode`/`__jet_Decode` trait signature. Body is lowered
/// through the same TIR as any trait method; only the header (name + receiver/params +
/// return) is the trait's, not the user's Jet-facing spelling.
///
/// - `Encode`: `fn jet_encode(&self) -> jet_std::DataTree { <body> }`. The user wrote
///   `fn encode(self) => Data`; bare `self` already lowers to `&self` and `Data` to
///   `jet_std::DataTree`, so only the method NAME is bridged.
/// - `Decode`: `fn jet_decode(<tree>: &jet_std::DataTree) -> Result<Self, Vec<jet_std::FieldError>>`.
///   The user wrote a STATIC `fn decode(tree: Data) => T ![FieldError]`; the by-value
///   `Data` param becomes a borrow with an owned clone re-bound at the head (`let <tree> =
///   <tree>.clone();`), so the body reads an owned `Data` local exactly as written.
pub(crate) fn emit_tir_serde_method(tir: &TFunc, codec: SerdeCodec, cx: &Cx, out: &mut String) {
    let name = match codec {
        SerdeCodec::Encode => "jet_encode",
        SerdeCodec::Decode => "jet_decode",
    };
    emit_tir_serde_method_named(tir, codec, cx, out, name);
}

/// Emit a serde method with a compiler-private name. Migration-aware decoders
/// use this only in an inherent helper impl; their one public trait entry point
/// remains `jet_decode` and the generated chain walker calls that entry point.
pub(crate) fn emit_tir_serde_method_named(
    tir: &TFunc,
    codec: SerdeCodec,
    cx: &Cx,
    out: &mut String,
    name: &str,
) {
    let indent = 1;
    let pad = "    ".repeat(indent);
    let scalar_attr = if tir.is_scalar {
        format!("{pad}#[inline(never)]\n")
    } else {
        String::new()
    };
    let auto_vectorization = auto_vectorization_attr(tir, indent);
    // E2-M12 D-OBS1: track the current function name for rich panic reports.
    *cx.current_fn.borrow_mut() = tir.name.clone();
    cx.current_fn_line
        .set(u32::try_from(tir.line).unwrap_or(u32::MAX));
    match codec {
        SerdeCodec::Encode => {
            out.push_str(&auto_vectorization);
            out.push_str(&scalar_attr);
            out.push_str(&format!(
                "{pad}fn {name}(&self) -> {}jet_std::DataTree {{\n",
                cx.root_prefix
            ));
            emit_stack_guard(tir, cx, out, indent + 1);
            emit_sentry_gate(tir, cx, out, indent + 1);
            emit_sentry_frame(tir, cx, out, indent + 1);
            if cx.coverage {
                out.push_str(&format!(
                    "{pad}    {}jet_cov({});\n",
                    cx.root_prefix, tir.line
                ));
            }
            let previous_scalar = cx.scalar_function.replace(tir.is_scalar);
            if !emit_tir_stmts_with_native_vectorization(&tir.body, cx, out, indent + 1) {
                emit_tir_stmts(&tir.body, cx, out, indent + 1);
            }
            cx.scalar_function.set(previous_scalar);
            out.push_str(&format!("{pad}}}\n"));
        }
        SerdeCodec::Decode => {
            // The single non-self param is the `tree: Data` argument. Render it as a
            // borrow and re-bind an owned clone so the lowered body (which reads the bare
            // name) sees an owned `Data`.
            let tree = tir
                .params
                .first()
                .map(|(n, _, _)| n.clone())
                .unwrap_or_else(|| "tree".to_string());
            let ret = match &tir.ret {
                Some(t) => rust_return_type(cx, t),
                None => format!("Result<Self, Vec<{}jet_std::FieldError>>", cx.root_prefix),
            };
            out.push_str(&auto_vectorization);
            out.push_str(&scalar_attr);
            out.push_str(&format!(
                "{pad}fn {name}({tree}: &{root}jet_std::DataTree) -> {ret} {{\n",
                root = cx.root_prefix
            ));
            emit_stack_guard(tir, cx, out, indent + 1);
            emit_sentry_gate(tir, cx, out, indent + 1);
            emit_sentry_frame(tir, cx, out, indent + 1);
            if cx.coverage {
                out.push_str(&format!(
                    "{pad}    {}jet_cov({});\n",
                    cx.root_prefix, tir.line
                ));
            }
            out.push_str(&format!("{pad}    let {tree} = ({tree}).clone();\n"));
            let previous_scalar = cx.scalar_function.replace(tir.is_scalar);
            if !emit_tir_stmts_with_native_vectorization(&tir.body, cx, out, indent + 1) {
                emit_tir_stmts(&tir.body, cx, out, indent + 1);
            }
            cx.scalar_function.set(previous_scalar);
            out.push_str(&format!("{pad}}}\n"));
        }
    }
}

/// c109 Phase 15: a DELEGATION trait method (`using field`), emitted INSIDE the
/// `impl Trait for __jet_<T> { … }` block `emit_external_trait_impl` opened. Byte-for-byte
/// `emit_delegation_method` (Source/Codegen/Items.rs): the pre-rendered signature line,
/// then the single forwarding call (`(self).<field>.<method>(args)`) at 8-space indent —
/// with a trailing `;` for a unit method, none for a returning one — then `    }`.
pub(crate) fn emit_tir_delegation(
    tir: &TFunc,
    sig: &str,
    fwd: &str,
    has_return: bool,
    cx: &Cx,
    out: &mut String,
) {
    // E2-M12 D-OBS1: track the current function name (parity with the AST path, though a
    // delegation body has no panic site of its own).
    *cx.current_fn.borrow_mut() = tir.name.clone();
    cx.current_fn_line
        .set(u32::try_from(tir.line).unwrap_or(u32::MAX));
    out.push_str(sig);
    if has_return {
        out.push_str(&format!("        {}\n", fwd));
    } else {
        out.push_str(&format!("        {};\n", fwd));
    }
    out.push_str("    }\n");
}
