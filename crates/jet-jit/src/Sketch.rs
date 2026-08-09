//! `core.sketch` marshalling hosts for the shared Prelude kernel.

use super::Concurrency;
use cranelift_codegen::ir::{types, AbiParam, Signature};
use cranelift_jit::{JITBuilder, JITModule};
use cranelift_module::{FuncId, Linkage, Module};
use crate::Marshal::clone_string;

pub(crate) mod sketch_rt {
    include!("../../jet-codegen/src/Prelude/Core/Sketch.rs");
}

#[derive(Clone)]
pub(crate) enum SketchSlot {
    Hll(sketch_rt::JetHyperLogLog),
    TDigest(sketch_rt::JetTDigest),
    Cms(sketch_rt::JetCountMinSketch),
    Reservoir(sketch_rt::JetReservoirSampler),
}

fn push_sketch(slot: SketchSlot) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        rt.sketches.push(slot);
        rt.sketches.len() as i64
    })
}

fn with_sketch_mut<R: Default>(handle: i64, f: impl FnOnce(&mut SketchSlot) -> R) -> R {
    Concurrency::with_runtime_mut(|rt| {
        let slot = rt
            .sketches
            .get_mut(handle.saturating_sub(1) as usize)
            .expect("jit sketch: bad handle");
        f(slot)
    })
}

fn list_from_strings(items: Vec<String>) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let list = rt.heap.alloc_empty_list();
        for s in items {
            let sid = rt.heap.alloc_string(s);
            rt.heap.list_push_int(list, sid).expect("jit sketch list");
        }
        list
    })
}

extern "C" fn jet_jit_hll_new() -> i64 {
    push_sketch(SketchSlot::Hll(sketch_rt::JetHyperLogLog::new()))
}

extern "C" fn jet_jit_tdigest_new() -> i64 {
    push_sketch(SketchSlot::TDigest(sketch_rt::JetTDigest::new()))
}

extern "C" fn jet_jit_cms_new() -> i64 {
    push_sketch(SketchSlot::Cms(sketch_rt::JetCountMinSketch::new()))
}

extern "C" fn jet_jit_reservoir_new(capacity: i64) -> i64 {
    push_sketch(SketchSlot::Reservoir(sketch_rt::JetReservoirSampler::new(
        capacity,
    )))
}

/// `kind`: 0=HLL, 1=TDigest (unused here), 2=CMS, 3=Reservoir.
extern "C" fn jet_jit_sketch_add_str(handle: i64, kind: i64, s: i64) {
    let item = clone_string(s);
    with_sketch_mut(handle, |slot| match (kind, slot) {
        (0, SketchSlot::Hll(h)) => h.add(&item),
        (2, SketchSlot::Cms(c)) => c.add(&item),
        (3, SketchSlot::Reservoir(r)) => r.add(item),
        _ => {}
    });
}

extern "C" fn jet_jit_sketch_add_f64(handle: i64, v: f64) {
    with_sketch_mut(handle, |slot| {
        if let SketchSlot::TDigest(td) = slot {
            td.add(v);
        }
    });
}

extern "C" fn jet_jit_sketch_count0(handle: i64) -> i64 {
    with_sketch_mut(handle, |slot| match slot {
        SketchSlot::Hll(h) => h.count(),
        _ => 0,
    })
}

extern "C" fn jet_jit_sketch_count1(handle: i64, key: i64) -> i64 {
    let k = clone_string(key);
    with_sketch_mut(handle, |slot| match slot {
        SketchSlot::Cms(c) => c.count(&k),
        _ => 0,
    })
}

extern "C" fn jet_jit_sketch_quantile(handle: i64, q: f64) -> f64 {
    with_sketch_mut(handle, |slot| match slot {
        SketchSlot::TDigest(td) => td.quantile(q),
        _ => 0.0,
    })
}

extern "C" fn jet_jit_sketch_sample(handle: i64) -> i64 {
    let items = with_sketch_mut(handle, |slot| -> Option<Vec<String>> {
        match slot {
            SketchSlot::Reservoir(r) => Some(r.sample()),
            _ => None,
        }
    });
    list_from_strings(items.unwrap_or_default())
}

host_fns! {
    struct SketchHostFns;
    register: register_sketch_symbols;
    declare: declare_sketch_host_fns(module) {
        let cc = module.target_config().default_call_conv;
        let mut nullary = Signature::new(cc);
        nullary.returns.push(AbiParam::new(types::I64));
        let mut unary = Signature::new(cc);
        unary.params.push(AbiParam::new(types::I64));
        unary.returns.push(AbiParam::new(types::I64));
        let mut unary_void = Signature::new(cc);
        unary_void.params.push(AbiParam::new(types::I64));
        unary_void.params.push(AbiParam::new(types::F64));
        let mut binary = Signature::new(cc);
        binary.params.push(AbiParam::new(types::I64));
        binary.params.push(AbiParam::new(types::I64));
        binary.returns.push(AbiParam::new(types::I64));
        let mut ternary_void = Signature::new(cc);
        ternary_void.params.push(AbiParam::new(types::I64));
        ternary_void.params.push(AbiParam::new(types::I64));
        ternary_void.params.push(AbiParam::new(types::I64));
        let mut quant = Signature::new(cc);
        quant.params.push(AbiParam::new(types::I64));
        quant.params.push(AbiParam::new(types::F64));
        quant.returns.push(AbiParam::new(types::F64));


    }
    hll_new: "jet_jit_hll_new" => jet_jit_hll_new: nullary;
    tdigest_new: "jet_jit_tdigest_new" => jet_jit_tdigest_new: nullary;
    cms_new: "jet_jit_cms_new" => jet_jit_cms_new: nullary;
    reservoir_new: "jet_jit_reservoir_new" => jet_jit_reservoir_new: unary;
    add_str: "jet_jit_sketch_add_str" => jet_jit_sketch_add_str: ternary_void;
    add_f64: "jet_jit_sketch_add_f64" => jet_jit_sketch_add_f64: unary_void;
    count0: "jet_jit_sketch_count0" => jet_jit_sketch_count0: unary;
    count1: "jet_jit_sketch_count1" => jet_jit_sketch_count1: binary;
    quantile: "jet_jit_sketch_quantile" => jet_jit_sketch_quantile: quant;
    sample: "jet_jit_sketch_sample" => jet_jit_sketch_sample: unary;
}




