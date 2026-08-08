//! D-LAYOUT1 / D-LAYOUT-CTOR1: resident-JIT host for `name :: Layout.{ … }` /
//! `Layout` — `include!`
//! canonical `jet_layout` (Prelude/Layout.rs). No third algorithm.

use super::Concurrency;
use crate::runtime_host::{alloc_jit_result, JitRuntime};
use cranelift_codegen::ir::{types, AbiParam, Signature};
use cranelift_jit::{JITBuilder, JITModule};
use cranelift_module::{FuncId, Linkage, Module};

/// Canonical Cassowary-style layout solver (extracted from Prelude/Layout.rs).
#[allow(dead_code, unused_imports)]
pub(crate) mod jet_layout {
    #[allow(unused_imports)]
    pub use jet_foundation::Outcome::*;
    include!(concat!(env!("OUT_DIR"), "/layout_rt.rs"));
}

#[derive(Clone)]
pub(crate) enum LayoutSlot {
    Handle(jet_layout::Handle),
    Expr(jet_layout::LinExpr),
    Constraint(jet_layout::Constraint),
}

fn with_rt<F, R>(f: F) -> R
where
    F: FnOnce(&mut JitRuntime) -> R,
    R: Default,
{
    Concurrency::with_runtime_mut(f)
}

fn push_slot(slot: LayoutSlot) -> i64 {
    with_rt(|rt| {
        rt.layout_slots.push(slot);
        rt.layout_slots.len() as i64
    })
}

fn slot(handle: i64) -> LayoutSlot {
    with_rt(|rt| {
        Some(
            rt.layout_slots
                .get(handle.saturating_sub(1) as usize)
                .expect("jit layout: bad handle")
                .clone(),
        )
    })
    .expect("jit layout: no active runtime")
}

extern "C" fn jet_jit_layout_new(label: i64) -> i64 {
    with_rt(|rt| {
        let label = rt.heap.clone_string(label).unwrap_or_else(|| "layout".into());
        let h = jet_layout::Handle::new(&label);
        rt.layout_slots.push(LayoutSlot::Handle(h));
        rt.layout_slots.len() as i64
    })
}

extern "C" fn jet_jit_layout_from_const(v: f64) -> i64 {
    push_slot(LayoutSlot::Expr(jet_layout::LinExpr::from_const(v)))
}

extern "C" fn jet_jit_layout_ge(lhs: i64, rhs: i64) -> i64 {
    match (slot(lhs), slot(rhs)) {
        (LayoutSlot::Expr(a), LayoutSlot::Expr(b)) => push_slot(LayoutSlot::Constraint(jet_layout::ge(a, b))),
        _ => jet_foundation::ice!(None, "jit layout ge: bad operands"),
    }
}

extern "C" fn jet_jit_layout_le(lhs: i64, rhs: i64) -> i64 {
    match (slot(lhs), slot(rhs)) {
        (LayoutSlot::Expr(a), LayoutSlot::Expr(b)) => push_slot(LayoutSlot::Constraint(jet_layout::le(a, b))),
        _ => jet_foundation::ice!(None, "jit layout le: bad operands"),
    }
}

extern "C" fn jet_jit_layout_eq(lhs: i64, rhs: i64) -> i64 {
    match (slot(lhs), slot(rhs)) {
        (LayoutSlot::Expr(a), LayoutSlot::Expr(b)) => push_slot(LayoutSlot::Constraint(jet_layout::eq_(a, b))),
        _ => jet_foundation::ice!(None, "jit layout eq: bad operands"),
    }
}

extern "C" fn jet_jit_layout_add(lhs: i64, rhs: i64) -> i64 {
    match (slot(lhs), slot(rhs)) {
        (LayoutSlot::Expr(a), LayoutSlot::Expr(b)) => push_slot(LayoutSlot::Expr(a + b)),
        _ => jet_foundation::ice!(None, "jit layout add: bad operands"),
    }
}

extern "C" fn jet_jit_layout_sub(lhs: i64, rhs: i64) -> i64 {
    match (slot(lhs), slot(rhs)) {
        (LayoutSlot::Expr(a), LayoutSlot::Expr(b)) => push_slot(LayoutSlot::Expr(a - b)),
        _ => jet_foundation::ice!(None, "jit layout sub: bad operands"),
    }
}

extern "C" fn jet_jit_layout_h(handle: i64, box_name: i64, anchor: i64) -> i64 {
    with_rt(|rt| {
        let box_name = rt.heap.clone_string(box_name).unwrap_or_default();
        let anchor = rt.heap.clone_string(anchor).unwrap_or_default();
        let LayoutSlot::Handle(h) = rt
            .layout_slots
            .get(handle.saturating_sub(1) as usize)
            .expect("jit layout h: bad handle")
            .clone()
        else {
            jet_foundation::ice!(None, "jit layout h: not a handle");
        };
        let expr = h.h(&box_name, &anchor);
        rt.layout_slots.push(LayoutSlot::Expr(expr));
        rt.layout_slots.len() as i64
    })
}

extern "C" fn jet_jit_layout_v(handle: i64, box_name: i64, anchor: i64) -> i64 {
    with_rt(|rt| {
        let box_name = rt.heap.clone_string(box_name).unwrap_or_default();
        let anchor = rt.heap.clone_string(anchor).unwrap_or_default();
        let LayoutSlot::Handle(h) = rt
            .layout_slots
            .get(handle.saturating_sub(1) as usize)
            .expect("jit layout v: bad handle")
            .clone()
        else {
            jet_foundation::ice!(None, "jit layout v: not a handle");
        };
        let expr = h.v(&box_name, &anchor);
        rt.layout_slots.push(LayoutSlot::Expr(expr));
        rt.layout_slots.len() as i64
    })
}

extern "C" fn jet_jit_layout_value(handle: i64, expr: i64) -> f64 {
    with_rt(|rt| {
        let LayoutSlot::Handle(h) = rt
            .layout_slots
            .get(handle.saturating_sub(1) as usize)
            .expect("jit layout value: bad handle")
            .clone()
        else {
            jet_foundation::ice!(None, "jit layout value: not a handle");
        };
        let LayoutSlot::Expr(e) = rt
            .layout_slots
            .get(expr.saturating_sub(1) as usize)
            .expect("jit layout value: bad expr")
            .clone()
        else {
            jet_foundation::ice!(None, "jit layout value: not an expr");
        };
        h.value(e)
    })
}

extern "C" fn jet_jit_layout_suggest(handle: i64, expr: i64, value: f64) {
    with_rt(|rt| {
        let LayoutSlot::Handle(h) = rt
            .layout_slots
            .get(handle.saturating_sub(1) as usize)
            .expect("jit layout suggest: bad handle")
            .clone()
        else {
            jet_foundation::ice!(None, "jit layout suggest: not a handle");
        };
        let LayoutSlot::Expr(e) = rt
            .layout_slots
            .get(expr.saturating_sub(1) as usize)
            .expect("jit layout suggest: bad expr")
            .clone()
        else {
            jet_foundation::ice!(None, "jit layout suggest: not an expr");
        };
        h.suggest(e, value);
    });
}

extern "C" fn jet_jit_layout_is_feasible(handle: i64) -> i8 {
    with_rt(|rt| {
        let LayoutSlot::Handle(h) = rt
            .layout_slots
            .get(handle.saturating_sub(1) as usize)
            .expect("jit layout is_feasible: bad handle")
            .clone()
        else {
            jet_foundation::ice!(None, "jit layout is_feasible: not a handle");
        };
        if h.is_feasible() {
            1
        } else {
            0
        }
    })
}

extern "C" fn jet_jit_layout_add_constraint(handle: i64, constraint: i64) {
    let _ = (handle, constraint);
    // Constraints auto-register via jet_layout::ge/le/eq_ (make_constraint).
}

extern "C" fn jet_jit_layout_strength(constraint: i64, kind: i64) -> i64 {
    with_rt(|rt| {
        let LayoutSlot::Constraint(c) = rt
            .layout_slots
            .get(constraint.saturating_sub(1) as usize)
            .expect("jit layout strength: bad constraint")
            .clone()
        else {
            jet_foundation::ice!(None, "jit layout strength: not a constraint");
        };
        let c = match kind {
            0 => c.required(),
            1 => c.strong(),
            2 => c.medium(),
            _ => c.weak(),
        };
        rt.layout_slots.push(LayoutSlot::Constraint(c));
        rt.layout_slots.len() as i64
    })
}

host_fns! {
    struct LayoutHostFns;
    register: register_layout_symbols;
    declare: declare_layout_host_fns(module) {
        let cc = module.target_config().default_call_conv;
        let mut unary = Signature::new(cc);
        unary.params.push(AbiParam::new(types::I64));
        unary.returns.push(AbiParam::new(types::I64));
        let mut binary = Signature::new(cc);
        binary.params.push(AbiParam::new(types::I64));
        binary.params.push(AbiParam::new(types::I64));
        binary.returns.push(AbiParam::new(types::I64));
        let mut from_const = Signature::new(cc);
        from_const.params.push(AbiParam::new(types::F64));
        from_const.returns.push(AbiParam::new(types::I64));
        let mut ternary = Signature::new(cc);
        ternary.params.push(AbiParam::new(types::I64));
        ternary.params.push(AbiParam::new(types::I64));
        ternary.params.push(AbiParam::new(types::I64));
        ternary.returns.push(AbiParam::new(types::I64));
        let mut value = Signature::new(cc);
        value.params.push(AbiParam::new(types::I64));
        value.params.push(AbiParam::new(types::I64));
        value.returns.push(AbiParam::new(types::F64));
        let mut suggest = Signature::new(cc);
        suggest.params.push(AbiParam::new(types::I64));
        suggest.params.push(AbiParam::new(types::I64));
        suggest.params.push(AbiParam::new(types::F64));
        let mut is_feasible = Signature::new(cc);
        is_feasible.params.push(AbiParam::new(types::I64));
        is_feasible.returns.push(AbiParam::new(types::I8));
        let mut add_c = Signature::new(cc);
        add_c.params.push(AbiParam::new(types::I64));
        add_c.params.push(AbiParam::new(types::I64));


    }
    new: "jet_jit_layout_new" => jet_jit_layout_new: unary;
    from_const: "jet_jit_layout_from_const" => jet_jit_layout_from_const: from_const;
    ge: "jet_jit_layout_ge" => jet_jit_layout_ge: binary;
    le: "jet_jit_layout_le" => jet_jit_layout_le: binary;
    eq: "jet_jit_layout_eq" => jet_jit_layout_eq: binary;
    add: "jet_jit_layout_add" => jet_jit_layout_add: binary;
    sub: "jet_jit_layout_sub" => jet_jit_layout_sub: binary;
    h: "jet_jit_layout_h" => jet_jit_layout_h: ternary;
    v: "jet_jit_layout_v" => jet_jit_layout_v: ternary;
    value: "jet_jit_layout_value" => jet_jit_layout_value: value;
    suggest: "jet_jit_layout_suggest" => jet_jit_layout_suggest: suggest;
    is_feasible: "jet_jit_layout_is_feasible" => jet_jit_layout_is_feasible: is_feasible;
    add_constraint: "jet_jit_layout_add_constraint" => jet_jit_layout_add_constraint: add_c;
    strength: "jet_jit_layout_strength" => jet_jit_layout_strength: binary;
}






// silence unused import warning until lower_ctx wires Result packing
#[allow(dead_code)]
fn _alloc(ok: bool, bits: u64) -> i64 {
    Concurrency::with_runtime_mut(|rt| alloc_jit_result(rt, ok, bits))
}
