//! `core.random` ambient + `Rng` handle host shims (#729).
//! Ambient and seeded draws call the shared Prelude kernels; this module only
//! marshals Cranelift ABI values and runtime-heap handles.

use super::Concurrency;

mod seeded_random_kernel {
    include!("../../jet-codegen/src/Prelude/Core/SeededRandom.rs");
}

#[allow(dead_code)]
mod ambient_random_kernel {
    pub(crate) mod jet_std {
        #[derive(Clone)]
        pub(crate) struct Rng {
            pub(crate) state: u64,
        }
    }

    include!("../../jet-codegen/src/Prelude/CoreLib/Top/MathRandomFns.rs");

    pub(crate) fn seed(seed: i64) {
        jet_std_random_seed(seed);
    }

    pub(crate) fn bool_p(p: f64) -> bool {
        jet_std_random_bool(p)
    }

    pub(crate) fn float_range(low: f64, high: f64) -> f64 {
        jet_std_random_float_range(low, high)
    }

    pub(crate) fn normal(mean: f64, stddev: f64) -> f64 {
        jet_std_random_normal(mean, stddev)
    }

    pub(crate) fn exponential(lambda: f64) -> f64 {
        jet_std_random_exponential(lambda)
    }

    pub(crate) fn bytes(count: i64) -> Vec<u8> {
        jet_std_random_bytes(count)
    }

    pub(crate) fn weighted_pick<T: Clone>(items: &Vec<T>, weights: &Vec<f64>) -> Option<T> {
        jet_std_random_weighted_pick(items, weights)
    }

    pub(crate) fn sample<T: Clone>(items: &Vec<T>, count: i64) -> Vec<T> {
        jet_std_random_sample(items, count)
    }
}

extern "C" fn jet_jit_random_seed(n: i64) {
    ambient_random_kernel::seed(n);
}

extern "C" fn jet_jit_random_bool(p: f64) -> i8 {
    i8::from(ambient_random_kernel::bool_p(p))
}

extern "C" fn jet_jit_random_float_range(low: f64, high: f64) -> f64 {
    ambient_random_kernel::float_range(low, high)
}

extern "C" fn jet_jit_random_normal(mean: f64, stddev: f64) -> f64 {
    ambient_random_kernel::normal(mean, stddev)
}

extern "C" fn jet_jit_random_exponential(lambda: f64) -> f64 {
    ambient_random_kernel::exponential(lambda)
}

extern "C" fn jet_jit_random_bytes(n: i64) -> i64 {
    let bytes = ambient_random_kernel::bytes(n);
    Concurrency::with_runtime_mut(|rt| {
        let list = rt.heap.alloc_empty_list();
        for byte in bytes {
            let _ = rt.heap.list_push_int(list, byte as i64);
        }
        list
    })
}

fn pack_option_string(opt: Option<i64>) -> i64 {
    match opt {
        Some(h) => h.wrapping_add(1),
        None => 0,
    }
}

extern "C" fn jet_jit_random_weighted_pick(items: i64, weights: i64) -> i64 {
    let Some((values, ws)) = Concurrency::with_runtime_mut(|rt| {
        let len = rt.heap.list_len(items).unwrap_or(0);
        if len == 0 || rt.heap.list_len(weights) != Some(len) {
            return None;
        }
        let values = (0..len)
            .map(|i| rt.heap.list_get_int(items, i).unwrap_or(0))
            .collect();
        let ws = (0..len)
            .map(|i| rt.heap.list_get_float(weights, i).unwrap_or(0.0))
            .collect();
        Some((values, ws))
    }) else {
        return pack_option_string(None);
    };
    pack_option_string(ambient_random_kernel::weighted_pick(&values, &ws))
}

extern "C" fn jet_jit_random_sample(items: i64, k: i64) -> i64 {
    let values = Concurrency::with_runtime_mut(|rt| {
        let len = rt.heap.list_len(items).unwrap_or(0);
        (0..len)
            .map(|i| rt.heap.list_get_int(items, i).unwrap_or(0))
            .collect::<Vec<_>>()
    });
    let sample = ambient_random_kernel::sample(&values, k);
    Concurrency::with_runtime_mut(|rt| {
        let out = rt.heap.alloc_empty_list();
        for sid in sample {
            let _ = rt.heap.list_push_int(out, sid);
        }
        out
    })
}

// ── Rng handle (MathRandomTime.rs SplitMix64) ────────────────────────────────

pub(crate) struct RngState {
    pub(crate) state: u64,
}

fn det_next(r: &mut RngState) -> u64 {
    seeded_random_kernel::jet_seeded_rng_next(&mut r.state)
}

fn rng_float(r: &mut RngState) -> f64 {
    seeded_random_kernel::jet_seeded_rng_float(&mut r.state)
}

fn rng_float_open(r: &mut RngState) -> f64 {
    seeded_random_kernel::jet_seeded_rng_float_open(&mut r.state)
}

fn rng_int(r: &mut RngState, lo: i64, hi: i64) -> i64 {
    seeded_random_kernel::jet_seeded_rng_int(&mut r.state, lo, hi)
}

fn with_rng<T: Default>(handle: i64, f: impl FnOnce(&mut RngState) -> T) -> T {
    Concurrency::with_runtime_mut(|rt| {
        let r = rt
            .rngs
            .get_mut(handle.saturating_sub(1) as usize)
            .expect("jit rng: bad handle");
        f(r)
    })
}

extern "C" fn jet_jit_rng_new(seed: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        rt.rngs.push(RngState {
            state: seed as u64,
        });
        rt.rngs.len() as i64
    })
}

extern "C" fn jet_jit_rng_int(handle: i64, lo: i64, hi: i64) -> i64 {
    with_rng(handle, |r| rng_int(r, lo, hi))
}

extern "C" fn jet_jit_rng_float_range(handle: i64, low: f64, high: f64) -> f64 {
    with_rng(handle, |r| {
        seeded_random_kernel::jet_seeded_rng_float_range(&mut r.state, low, high)
    })
}

extern "C" fn jet_jit_rng_bool_p(handle: i64, p: f64) -> i8 {
    with_rng(handle, |r| {
        i8::from(seeded_random_kernel::jet_seeded_rng_bool_p(&mut r.state, p))
    })
}

extern "C" fn jet_jit_rng_bool(handle: i64) -> i8 {
    // Match AOT `jet_rng_bool` / comptime seeded rng: LSB of SplitMix64 next.
    with_rng(handle, |r| {
        i8::from(seeded_random_kernel::jet_seeded_rng_bool(&mut r.state))
    })
}

extern "C" fn jet_jit_rng_pick(handle: i64, items: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let len = rt.heap.list_len(items).unwrap_or(0);
        if len == 0 {
            return pack_option_string(None);
        }
        let index = {
            let rng = rt
                .rngs
                .get_mut(handle.saturating_sub(1) as usize)
                .expect("jit rng pick: bad handle");
            rng_int(rng, 0, len - 1)
        };
        pack_option_string(rt.heap.list_get_int(items, index))
    })
}

extern "C" fn jet_jit_rng_shuffle(handle: i64, items: i64) {
    Concurrency::with_runtime_mut(|rt| {
        let len = rt.heap.list_len(items).unwrap_or(0);
        for i in (1..len).rev() {
            let j = {
                let rng = rt
                    .rngs
                    .get_mut(handle.saturating_sub(1) as usize)
                    .expect("jit rng shuffle: bad handle");
                rng_int(rng, 0, i)
            };
            let a = rt.heap.list_get_int(items, i).unwrap_or(0);
            let b = rt.heap.list_get_int(items, j).unwrap_or(0);
            let _ = rt.heap.list_set_int(items, i, b);
            let _ = rt.heap.list_set_int(items, j, a);
        }
    });
}

extern "C" fn jet_jit_rng_weighted_pick(handle: i64, items: i64, weights: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let len = rt.heap.list_len(items).unwrap_or(0);
        if len == 0 || rt.heap.list_len(weights) != Some(len) {
            return pack_option_string(None);
        }
        let mut total = 0.0;
        let mut ws = Vec::with_capacity(len as usize);
        for i in 0..len {
            let w = rt.heap.list_get_float(weights, i).unwrap_or(0.0);
            let w = if w.is_finite() && w > 0.0 { w } else { 0.0 };
            total += w;
            ws.push(w);
        }
        if total <= 0.0 {
            return pack_option_string(None);
        }
        let r = rt
            .rngs
            .get_mut(handle.saturating_sub(1) as usize)
            .expect("jit rng weighted_pick: bad handle");
        let mut needle = {
            let low = 0.0;
            let high = total;
            if !(high > low) {
                low
            } else {
                low + (high - low) * rng_float(r)
            }
        };
        for i in 0..len {
            let w = ws[i as usize];
            if needle < w {
                let sid = rt.heap.list_get_int(items, i).unwrap_or(0);
                return pack_option_string(Some(sid));
            }
            needle -= w;
        }
        let sid = rt.heap.list_get_int(items, len - 1).unwrap_or(0);
        pack_option_string(Some(sid))
    })
}

extern "C" fn jet_jit_rng_sample(handle: i64, items: i64, k: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let len = rt.heap.list_len(items).unwrap_or(0) as usize;
        let want = (k.max(0) as usize).min(len);
        let mut pool: Vec<i64> = (0..len as i64)
            .map(|i| rt.heap.list_get_int(items, i).unwrap_or(0))
            .collect();
        {
            let r = rt
                .rngs
                .get_mut(handle.saturating_sub(1) as usize)
                .expect("jit rng sample: bad handle");
            for i in 0..want {
                let j = rng_int(r, i as i64, pool.len() as i64 - 1) as usize;
                pool.swap(i, j);
            }
        }
        pool.truncate(want);
        let out = rt.heap.alloc_empty_list();
        for sid in pool {
            let _ = rt.heap.list_push_int(out, sid);
        }
        out
    })
}

extern "C" fn jet_jit_rng_bytes(handle: i64, n: i64) -> i64 {
    let n = n.max(0) as usize;
    Concurrency::with_runtime_mut(|rt| {
        let list = rt.heap.alloc_empty_list();
        let r = rt
            .rngs
            .get_mut(handle.saturating_sub(1) as usize)
            .expect("jit rng bytes: bad handle");
        for _ in 0..n {
            let b = det_next(r) as u8;
            let _ = rt.heap.list_push_int(list, b as i64);
        }
        list
    })
}

extern "C" fn jet_jit_rng_split(handle: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let state = {
            let r = rt
                .rngs
                .get_mut(handle.saturating_sub(1) as usize)
                .expect("jit rng split: bad handle");
            det_next(r)
        };
        rt.rngs.push(RngState { state });
        rt.rngs.len() as i64
    })
}

host_fns! {
    struct RandomHostFns;
    register: register_random_symbols;
    declare: declare_random_host_fns(module) {
        use cranelift_codegen::ir::{types, AbiParam, Signature};
        use cranelift_module::{Linkage, Module};
        let cc = module.target_config().default_call_conv;

        let mut sig_void_i64 = Signature::new(cc);
        sig_void_i64.params.push(AbiParam::new(types::I64));
        let mut sig_void_i64_i64 = Signature::new(cc);
        sig_void_i64_i64.params.push(AbiParam::new(types::I64));
        sig_void_i64_i64.params.push(AbiParam::new(types::I64));
        let mut sig_f64_i8 = Signature::new(cc);
        sig_f64_i8.params.push(AbiParam::new(types::F64));
        sig_f64_i8.returns.push(AbiParam::new(types::I8));
        let mut sig_f64_f64_f64 = Signature::new(cc);
        sig_f64_f64_f64.params.push(AbiParam::new(types::F64));
        sig_f64_f64_f64.params.push(AbiParam::new(types::F64));
        sig_f64_f64_f64.returns.push(AbiParam::new(types::F64));
        let mut sig_f64 = Signature::new(cc);
        sig_f64.params.push(AbiParam::new(types::F64));
        sig_f64.returns.push(AbiParam::new(types::F64));
        let mut sig_i64_i64 = Signature::new(cc);
        sig_i64_i64.params.push(AbiParam::new(types::I64));
        sig_i64_i64.returns.push(AbiParam::new(types::I64));
        let mut sig_i64_i64_i64 = Signature::new(cc);
        sig_i64_i64_i64.params.push(AbiParam::new(types::I64));
        sig_i64_i64_i64.params.push(AbiParam::new(types::I64));
        sig_i64_i64_i64.returns.push(AbiParam::new(types::I64));
        let mut sig_i64_i64_i64_i64 = Signature::new(cc);
        sig_i64_i64_i64_i64.params.push(AbiParam::new(types::I64));
        sig_i64_i64_i64_i64.params.push(AbiParam::new(types::I64));
        sig_i64_i64_i64_i64.params.push(AbiParam::new(types::I64));
        sig_i64_i64_i64_i64.returns.push(AbiParam::new(types::I64));
        let mut sig_rng_fr = Signature::new(cc);
        sig_rng_fr.params.push(AbiParam::new(types::I64));
        sig_rng_fr.params.push(AbiParam::new(types::F64));
        sig_rng_fr.params.push(AbiParam::new(types::F64));
        sig_rng_fr.returns.push(AbiParam::new(types::F64));
        let mut sig_rng_bool = Signature::new(cc);
        sig_rng_bool.params.push(AbiParam::new(types::I64));
        sig_rng_bool.params.push(AbiParam::new(types::F64));
        sig_rng_bool.returns.push(AbiParam::new(types::I8));
        let mut sig_rng_bool_default = Signature::new(cc);
        sig_rng_bool_default.params.push(AbiParam::new(types::I64));
        sig_rng_bool_default.returns.push(AbiParam::new(types::I8));

    }
    seed: "jet_jit_random_seed" => jet_jit_random_seed: sig_void_i64;
    bool_p: "jet_jit_random_bool" => jet_jit_random_bool: sig_f64_i8;
    float_range: "jet_jit_random_float_range" => jet_jit_random_float_range: sig_f64_f64_f64;
    normal: "jet_jit_random_normal" => jet_jit_random_normal: sig_f64_f64_f64;
    exponential: "jet_jit_random_exponential" => jet_jit_random_exponential: sig_f64;
    bytes: "jet_jit_random_bytes" => jet_jit_random_bytes: sig_i64_i64;
    weighted_pick: "jet_jit_random_weighted_pick" => jet_jit_random_weighted_pick: sig_i64_i64_i64;
    sample: "jet_jit_random_sample" => jet_jit_random_sample: sig_i64_i64_i64;
    rng_new: "jet_jit_rng_new" => jet_jit_rng_new: sig_i64_i64;
    rng_int: "jet_jit_rng_int" => jet_jit_rng_int: sig_i64_i64_i64_i64;
    rng_float_range: "jet_jit_rng_float_range" => jet_jit_rng_float_range: sig_rng_fr;
    rng_bool: "jet_jit_rng_bool" => jet_jit_rng_bool: sig_rng_bool_default;
    rng_bool_p: "jet_jit_rng_bool_p" => jet_jit_rng_bool_p: sig_rng_bool;
    rng_pick: "jet_jit_rng_pick" => jet_jit_rng_pick: sig_i64_i64_i64;
    rng_shuffle: "jet_jit_rng_shuffle" => jet_jit_rng_shuffle: sig_void_i64_i64;
    rng_weighted_pick: "jet_jit_rng_weighted_pick" => jet_jit_rng_weighted_pick: sig_i64_i64_i64_i64;
    rng_sample: "jet_jit_rng_sample" => jet_jit_rng_sample: sig_i64_i64_i64_i64;
    rng_bytes: "jet_jit_rng_bytes" => jet_jit_rng_bytes: sig_i64_i64_i64;
    rng_split: "jet_jit_rng_split" => jet_jit_rng_split: sig_i64_i64;
}

