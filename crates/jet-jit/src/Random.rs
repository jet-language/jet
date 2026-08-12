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

    pub(crate) fn int(low: i64, high: i64) -> i64 {
        jet_std_random_int(low, high)
    }

    pub(crate) fn float() -> f64 {
        jet_std_random_float()
    }

    pub(crate) fn pick<T: Clone>(items: &Vec<T>) -> Option<T> {
        jet_std_random_pick(items)
    }

    pub(crate) fn shuffle<T>(items: &mut Vec<T>) {
        jet_std_random_shuffle(items);
    }

    pub(crate) fn split(seed: i64) -> jet_std::Rng {
        jet_std_random_split(seed)
    }
}

pub(crate) fn ambient_seed(seed: i64) {
    ambient_random_kernel::seed(seed);
}

pub(crate) fn ambient_int(low: i64, high: i64) -> i64 {
    ambient_random_kernel::int(low, high)
}

pub(crate) fn ambient_float() -> f64 {
    ambient_random_kernel::float()
}

pub(crate) fn ambient_float_range(low: f64, high: f64) -> f64 {
    ambient_random_kernel::float_range(low, high)
}

pub(crate) fn ambient_bool(p: f64) -> bool {
    ambient_random_kernel::bool_p(p)
}

pub(crate) fn ambient_normal(mean: f64, stddev: f64) -> f64 {
    ambient_random_kernel::normal(mean, stddev)
}

pub(crate) fn ambient_exponential(lambda: f64) -> f64 {
    ambient_random_kernel::exponential(lambda)
}

pub(crate) fn ambient_bytes(count: i64) -> Vec<u8> {
    ambient_random_kernel::bytes(count)
}

pub(crate) fn ambient_pick<T: Clone>(items: &Vec<T>) -> Option<T> {
    ambient_random_kernel::pick(items)
}

pub(crate) fn ambient_weighted_pick<T: Clone>(
    items: &Vec<T>,
    weights: &Vec<f64>,
) -> Option<T> {
    ambient_random_kernel::weighted_pick(items, weights)
}

pub(crate) fn ambient_sample<T: Clone>(items: &Vec<T>, count: i64) -> Vec<T> {
    ambient_random_kernel::sample(items, count)
}

pub(crate) fn ambient_shuffle<T>(items: &mut Vec<T>) {
    ambient_random_kernel::shuffle(items);
}

pub(crate) fn ambient_split(seed: i64) -> i64 {
    ambient_random_kernel::split(seed).state as i64
}

fn read_list<T>(list: i64, read: impl Fn(&jet_rt::JetArena, i64) -> Option<T>) -> Option<Vec<T>> {
    Concurrency::with_runtime_mut(|rt| {
        let len = rt.heap.list_len(list)?;
        (0..len).map(|index| read(&rt.heap, index)).collect()
    })
}

fn write_list<T>(list: i64, values: Vec<T>, write: impl Fn(&mut jet_rt::JetArena, i64, T) -> Option<()>) {
    Concurrency::with_runtime_mut(|rt| {
        for (index, value) in values.into_iter().enumerate() {
            let _ = write(&mut rt.heap, index as i64, value);
        }
    });
}

fn alloc_list<T>(values: Vec<T>, push: impl Fn(&mut jet_rt::JetArena, i64, T) -> Option<()>) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let list = rt.heap.alloc_empty_list();
        for value in values {
            let _ = push(&mut rt.heap, list, value);
        }
        list
    })
}

fn list_is_float(list: i64) -> bool {
    Concurrency::with_runtime_mut(|rt| {
        rt.heap.list_len(list).is_some_and(|len| len > 0)
            && rt.heap.list_get_float(list, 0).is_some()
    })
}

extern "C" fn jet_jit_random_seed(n: i64) {
    ambient_random_kernel::seed(n);
}

extern "C" fn jet_jit_random_int(low: i64, high: i64) -> i64 {
    ambient_random_kernel::int(low, high)
}

extern "C" fn jet_jit_random_float() -> f64 {
    ambient_random_kernel::float()
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

extern "C" fn jet_jit_random_pick(items: i64) -> i64 {
    if list_is_float(items) {
        let values = read_list(items, |heap, index| heap.list_get_float(items, index))
            .unwrap_or_default();
        pack_option_float(ambient_random_kernel::pick(&values))
    } else {
        let values = read_list(items, |heap, index| heap.list_get_int(items, index))
            .unwrap_or_default();
        pack_option_i64(ambient_random_kernel::pick(&values))
    }
}

extern "C" fn jet_jit_random_shuffle(items: i64) {
    if list_is_float(items) {
        let mut values = read_list(items, |heap, index| heap.list_get_float(items, index))
            .unwrap_or_default();
        ambient_random_kernel::shuffle(&mut values);
        write_list(items, values, |heap, index, value| {
            heap.list_set_float(items, index, value)
        });
    } else {
        let mut values = read_list(items, |heap, index| heap.list_get_int(items, index))
            .unwrap_or_default();
        ambient_random_kernel::shuffle(&mut values);
        write_list(items, values, |heap, index, value| {
            heap.list_set_int(items, index, value)
        });
    }
}

extern "C" fn jet_jit_random_split(seed: i64) -> i64 {
    let state = ambient_random_kernel::split(seed).state;
    Concurrency::with_runtime_mut(|rt| {
        rt.rngs.push(RngState { state });
        rt.rngs.len() as i64
    })
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

fn pack_option_i64(opt: Option<i64>) -> i64 {
    match opt {
        Some(h) => h.wrapping_add(1),
        None => 0,
    }
}

fn pack_option_float(opt: Option<f64>) -> i64 {
    match opt {
        Some(value) => (value.to_bits() as i64).wrapping_add(1),
        None => 0,
    }
}

extern "C" fn jet_jit_random_weighted_pick(items: i64, weights: i64) -> i64 {
    let Some(ws) = read_list(weights, |heap, index| heap.list_get_float(weights, index)) else {
        return pack_option_i64(None);
    };
    if list_is_float(items) {
        let Some(values) = read_list(items, |heap, index| heap.list_get_float(items, index)) else {
            return pack_option_float(None);
        };
        pack_option_float(ambient_random_kernel::weighted_pick(&values, &ws))
    } else {
        let Some(values) = read_list(items, |heap, index| heap.list_get_int(items, index)) else {
            return pack_option_i64(None);
        };
        pack_option_i64(ambient_random_kernel::weighted_pick(&values, &ws))
    }
}

extern "C" fn jet_jit_random_sample(items: i64, k: i64) -> i64 {
    if list_is_float(items) {
        let values = read_list(items, |heap, index| heap.list_get_float(items, index))
            .unwrap_or_default();
        let sample = ambient_random_kernel::sample(&values, k);
        alloc_list(sample, |heap, list, value| heap.list_push_float(list, value))
    } else {
        let values = read_list(items, |heap, index| heap.list_get_int(items, index))
            .unwrap_or_default();
        let sample = ambient_random_kernel::sample(&values, k);
        alloc_list(sample, |heap, list, value| heap.list_push_int(list, value))
    }
}

// ── Rng handle (MathRandomTime.rs SplitMix64) ────────────────────────────────

pub(crate) struct RngState {
    pub(crate) state: u64,
}

fn rng_float(r: &mut RngState) -> f64 {
    seeded_random_kernel::jet_seeded_rng_float(&mut r.state)
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

extern "C" fn jet_jit_rng_float(handle: i64) -> f64 {
    with_rng(handle, rng_float)
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

extern "C" fn jet_jit_rng_normal(handle: i64, mean: f64, stddev: f64) -> f64 {
    with_rng(handle, |r| {
        seeded_random_kernel::jet_seeded_rng_normal(&mut r.state, mean, stddev)
    })
}

extern "C" fn jet_jit_rng_exponential(handle: i64, lambda: f64) -> f64 {
    with_rng(handle, |r| {
        seeded_random_kernel::jet_seeded_rng_exponential(&mut r.state, lambda)
    })
}

extern "C" fn jet_jit_rng_pick(handle: i64, items: i64) -> i64 {
    if list_is_float(items) {
        let values = read_list(items, |heap, index| heap.list_get_float(items, index))
            .unwrap_or_default();
        let value = with_rng(handle, |rng| {
            seeded_random_kernel::jet_seeded_rng_pick(&mut rng.state, &values)
        });
        pack_option_float(value)
    } else {
        let values = read_list(items, |heap, index| heap.list_get_int(items, index))
            .unwrap_or_default();
        let value = with_rng(handle, |rng| {
            seeded_random_kernel::jet_seeded_rng_pick(&mut rng.state, &values)
        });
        pack_option_i64(value)
    }
}

extern "C" fn jet_jit_rng_shuffle(handle: i64, items: i64) {
    if list_is_float(items) {
        let mut values = read_list(items, |heap, index| heap.list_get_float(items, index))
            .unwrap_or_default();
        with_rng(handle, |rng| {
            seeded_random_kernel::jet_seeded_rng_shuffle(&mut rng.state, &mut values)
        });
        write_list(items, values, |heap, index, value| {
            heap.list_set_float(items, index, value)
        });
    } else {
        let mut values = read_list(items, |heap, index| heap.list_get_int(items, index))
            .unwrap_or_default();
        with_rng(handle, |rng| {
            seeded_random_kernel::jet_seeded_rng_shuffle(&mut rng.state, &mut values)
        });
        write_list(items, values, |heap, index, value| {
            heap.list_set_int(items, index, value)
        });
    }
}

extern "C" fn jet_jit_rng_weighted_pick(handle: i64, items: i64, weights: i64) -> i64 {
    let Some(ws) = read_list(weights, |heap, index| heap.list_get_float(weights, index)) else {
        return pack_option_i64(None);
    };
    if list_is_float(items) {
        let Some(values) = read_list(items, |heap, index| heap.list_get_float(items, index)) else {
            return pack_option_float(None);
        };
        let value = with_rng(handle, |rng| {
            seeded_random_kernel::jet_seeded_rng_weighted_pick(&mut rng.state, &values, &ws)
        });
        pack_option_float(value)
    } else {
        let Some(values) = read_list(items, |heap, index| heap.list_get_int(items, index)) else {
            return pack_option_i64(None);
        };
        let value = with_rng(handle, |rng| {
            seeded_random_kernel::jet_seeded_rng_weighted_pick(&mut rng.state, &values, &ws)
        });
        pack_option_i64(value)
    }
}

extern "C" fn jet_jit_rng_sample(handle: i64, items: i64, k: i64) -> i64 {
    if list_is_float(items) {
        let values = read_list(items, |heap, index| heap.list_get_float(items, index))
            .unwrap_or_default();
        let sample = with_rng(handle, |rng| {
            seeded_random_kernel::jet_seeded_rng_sample(&mut rng.state, &values, k)
        });
        alloc_list(sample, |heap, list, value| heap.list_push_float(list, value))
    } else {
        let values = read_list(items, |heap, index| heap.list_get_int(items, index))
            .unwrap_or_default();
        let sample = with_rng(handle, |rng| {
            seeded_random_kernel::jet_seeded_rng_sample(&mut rng.state, &values, k)
        });
        alloc_list(sample, |heap, list, value| heap.list_push_int(list, value))
    }
}

extern "C" fn jet_jit_rng_bytes(handle: i64, n: i64) -> i64 {
    let n = n.max(0);
    Concurrency::with_runtime_mut(|rt| {
        let list = rt.heap.alloc_empty_list();
        let r = rt
            .rngs
            .get_mut(handle.saturating_sub(1) as usize)
            .expect("jit rng bytes: bad handle");
        for b in seeded_random_kernel::jet_seeded_rng_bytes(&mut r.state, n) {
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
            seeded_random_kernel::jet_seeded_rng_split(&mut r.state)
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
        let mut sig_noarg_f64 = Signature::new(cc);
        sig_noarg_f64.returns.push(AbiParam::new(types::F64));
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
        let mut sig_rng_f = Signature::new(cc);
        sig_rng_f.params.push(AbiParam::new(types::I64));
        sig_rng_f.returns.push(AbiParam::new(types::F64));
        let mut sig_rng_exp = Signature::new(cc);
        sig_rng_exp.params.push(AbiParam::new(types::I64));
        sig_rng_exp.params.push(AbiParam::new(types::F64));
        sig_rng_exp.returns.push(AbiParam::new(types::F64));
        let mut sig_rng_bool = Signature::new(cc);
        sig_rng_bool.params.push(AbiParam::new(types::I64));
        sig_rng_bool.params.push(AbiParam::new(types::F64));
        sig_rng_bool.returns.push(AbiParam::new(types::I8));
        let mut sig_rng_bool_default = Signature::new(cc);
        sig_rng_bool_default.params.push(AbiParam::new(types::I64));
        sig_rng_bool_default.returns.push(AbiParam::new(types::I8));

    }
    seed: "jet_jit_random_seed" => jet_jit_random_seed: sig_void_i64;
    int: "jet_jit_random_int" => jet_jit_random_int: sig_i64_i64_i64;
    float: "jet_jit_random_float" => jet_jit_random_float: sig_noarg_f64;
    bool_p: "jet_jit_random_bool" => jet_jit_random_bool: sig_f64_i8;
    float_range: "jet_jit_random_float_range" => jet_jit_random_float_range: sig_f64_f64_f64;
    normal: "jet_jit_random_normal" => jet_jit_random_normal: sig_f64_f64_f64;
    exponential: "jet_jit_random_exponential" => jet_jit_random_exponential: sig_f64;
    bytes: "jet_jit_random_bytes" => jet_jit_random_bytes: sig_i64_i64;
    pick: "jet_jit_random_pick" => jet_jit_random_pick: sig_i64_i64;
    shuffle: "jet_jit_random_shuffle" => jet_jit_random_shuffle: sig_void_i64;
    split: "jet_jit_random_split" => jet_jit_random_split: sig_i64_i64;
    weighted_pick: "jet_jit_random_weighted_pick" => jet_jit_random_weighted_pick: sig_i64_i64_i64;
    sample: "jet_jit_random_sample" => jet_jit_random_sample: sig_i64_i64_i64;
    rng_new: "jet_jit_rng_new" => jet_jit_rng_new: sig_i64_i64;
    rng_int: "jet_jit_rng_int" => jet_jit_rng_int: sig_i64_i64_i64_i64;
    rng_float: "jet_jit_rng_float" => jet_jit_rng_float: sig_rng_f;
    rng_float_range: "jet_jit_rng_float_range" => jet_jit_rng_float_range: sig_rng_fr;
    rng_bool: "jet_jit_rng_bool" => jet_jit_rng_bool: sig_rng_bool_default;
    rng_bool_p: "jet_jit_rng_bool_p" => jet_jit_rng_bool_p: sig_rng_bool;
    rng_normal: "jet_jit_rng_normal" => jet_jit_rng_normal: sig_rng_fr;
    rng_exponential: "jet_jit_rng_exponential" => jet_jit_rng_exponential: sig_rng_exp;
    rng_pick: "jet_jit_rng_pick" => jet_jit_rng_pick: sig_i64_i64_i64;
    rng_shuffle: "jet_jit_rng_shuffle" => jet_jit_rng_shuffle: sig_void_i64_i64;
    rng_weighted_pick: "jet_jit_rng_weighted_pick" => jet_jit_rng_weighted_pick: sig_i64_i64_i64_i64;
    rng_sample: "jet_jit_rng_sample" => jet_jit_rng_sample: sig_i64_i64_i64_i64;
    rng_bytes: "jet_jit_rng_bytes" => jet_jit_rng_bytes: sig_i64_i64_i64;
    rng_split: "jet_jit_rng_split" => jet_jit_rng_split: sig_i64_i64;
}
