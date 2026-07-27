//! `core.random` ambient + `Rng` handle host shims (#729).
//! Ambient mirrors `jet_std_random_*` in Process.rs; `Rng` mirrors SplitMix64
//! `jet_rng_*` / `jet_det_rng_next` in MathRandomTime.rs — no third algorithm.

use super::Concurrency;

// ── ambient PRNG (Process.rs jet_rng_next / jet_std_random_*) ────────────────

thread_local! {
    static JIT_AMBIENT_RNG: std::cell::Cell<u64> =
        const { std::cell::Cell::new(0x4d595df4d0f33173) };
}

fn ambient_next() -> u64 {
    JIT_AMBIENT_RNG.with(|cell| {
        let mut x = cell.get();
        x ^= x << 7;
        x ^= x >> 9;
        x = x.wrapping_mul(0x9e3779b97f4a7c15);
        cell.set(x);
        x
    })
}

fn ambient_float() -> f64 {
    (ambient_next() as f64) / (u64::MAX as f64)
}

fn ambient_float_open() -> f64 {
    let x = ambient_float();
    if x <= 0.0 {
        f64::MIN_POSITIVE
    } else {
        x
    }
}

fn ambient_int(low: i64, high: i64) -> i64 {
    if high <= low {
        return low;
    }
    low + (ambient_next() % ((high - low + 1) as u64)) as i64
}

extern "C" fn jet_jit_random_seed(n: i64) {
    JIT_AMBIENT_RNG.with(|cell| cell.set(n as u64));
}

extern "C" fn jet_jit_random_bool(p: f64) -> i8 {
    let ok = if p <= 0.0 || p.is_nan() {
        false
    } else if p >= 1.0 {
        true
    } else {
        ambient_float() < p
    };
    i8::from(ok)
}

extern "C" fn jet_jit_random_float_range(low: f64, high: f64) -> f64 {
    if !(high > low) {
        return low;
    }
    low + (high - low) * ambient_float()
}

extern "C" fn jet_jit_random_normal(mean: f64, stddev: f64) -> f64 {
    let u1 = ambient_float_open();
    let u2 = ambient_float();
    let z0 = (-2.0 * u1.ln()).sqrt() * (std::f64::consts::TAU * u2).cos();
    mean + z0 * stddev.max(0.0)
}

extern "C" fn jet_jit_random_exponential(lambda: f64) -> f64 {
    if lambda <= 0.0 || lambda.is_nan() {
        return 0.0;
    }
    -ambient_float_open().ln() / lambda
}

extern "C" fn jet_jit_random_bytes(n: i64) -> i64 {
    let n = n.max(0) as usize;
    Concurrency::with_runtime_mut(|rt| {
        let list = rt.heap.alloc_empty_list();
        for _ in 0..n {
            let _ = rt.heap.list_push_int(list, (ambient_next() as u8) as i64);
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
        let mut needle = {
            let low = 0.0;
            let high = total;
            if !(high > low) {
                low
            } else {
                low + (high - low) * ambient_float()
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

extern "C" fn jet_jit_random_sample(items: i64, k: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let len = rt.heap.list_len(items).unwrap_or(0) as usize;
        let want = (k.max(0) as usize).min(len);
        let mut pool: Vec<i64> = (0..len as i64)
            .map(|i| rt.heap.list_get_int(items, i).unwrap_or(0))
            .collect();
        for i in 0..want {
            let j = ambient_int(i as i64, pool.len() as i64 - 1) as usize;
            pool.swap(i, j);
        }
        pool.truncate(want);
        let out = rt.heap.alloc_empty_list();
        for sid in pool {
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
    r.state = r.state.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut z = r.state;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

fn rng_float(r: &mut RngState) -> f64 {
    (det_next(r) >> 11) as f64 / (1u64 << 53) as f64
}

fn rng_float_open(r: &mut RngState) -> f64 {
    let x = rng_float(r);
    if x <= 0.0 {
        f64::MIN_POSITIVE
    } else {
        x
    }
}

fn rng_int(r: &mut RngState, lo: i64, hi: i64) -> i64 {
    if hi <= lo {
        return lo;
    }
    let span = (hi - lo + 1) as u64;
    lo + (det_next(r) % span) as i64
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
        if !(high > low) {
            return low;
        }
        low + (high - low) * rng_float(r)
    })
}

extern "C" fn jet_jit_rng_bool_p(handle: i64, p: f64) -> i8 {
    with_rng(handle, |r| {
        let ok = if p <= 0.0 || p.is_nan() {
            false
        } else if p >= 1.0 {
            true
        } else {
            rng_float(r) < p
        };
        i8::from(ok)
    })
}

extern "C" fn jet_jit_rng_bool(handle: i64) -> i8 {
    // Match AOT `jet_rng_bool` / comptime seeded rng: LSB of SplitMix64 next.
    with_rng(handle, |r| i8::from((det_next(r) & 1) == 1))
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

pub(crate) struct RandomHostFns {
    pub seed: cranelift_module::FuncId,
    pub bool_p: cranelift_module::FuncId,
    pub float_range: cranelift_module::FuncId,
    pub normal: cranelift_module::FuncId,
    pub exponential: cranelift_module::FuncId,
    pub bytes: cranelift_module::FuncId,
    pub weighted_pick: cranelift_module::FuncId,
    pub sample: cranelift_module::FuncId,
    pub rng_new: cranelift_module::FuncId,
    pub rng_int: cranelift_module::FuncId,
    pub rng_float_range: cranelift_module::FuncId,
    pub rng_bool: cranelift_module::FuncId,
    pub rng_bool_p: cranelift_module::FuncId,
    pub rng_pick: cranelift_module::FuncId,
    pub rng_shuffle: cranelift_module::FuncId,
    pub rng_weighted_pick: cranelift_module::FuncId,
    pub rng_sample: cranelift_module::FuncId,
    pub rng_bytes: cranelift_module::FuncId,
    pub rng_split: cranelift_module::FuncId,
}

pub(crate) fn register_random_symbols(builder: &mut cranelift_jit::JITBuilder) {
    builder.symbol("jet_jit_random_seed", jet_jit_random_seed as *const u8);
    builder.symbol("jet_jit_random_bool", jet_jit_random_bool as *const u8);
    builder.symbol(
        "jet_jit_random_float_range",
        jet_jit_random_float_range as *const u8,
    );
    builder.symbol("jet_jit_random_normal", jet_jit_random_normal as *const u8);
    builder.symbol(
        "jet_jit_random_exponential",
        jet_jit_random_exponential as *const u8,
    );
    builder.symbol("jet_jit_random_bytes", jet_jit_random_bytes as *const u8);
    builder.symbol(
        "jet_jit_random_weighted_pick",
        jet_jit_random_weighted_pick as *const u8,
    );
    builder.symbol("jet_jit_random_sample", jet_jit_random_sample as *const u8);
    builder.symbol("jet_jit_rng_new", jet_jit_rng_new as *const u8);
    builder.symbol("jet_jit_rng_int", jet_jit_rng_int as *const u8);
    builder.symbol(
        "jet_jit_rng_float_range",
        jet_jit_rng_float_range as *const u8,
    );
    builder.symbol("jet_jit_rng_bool_p", jet_jit_rng_bool_p as *const u8);
    builder.symbol("jet_jit_rng_bool", jet_jit_rng_bool as *const u8);
    builder.symbol("jet_jit_rng_pick", jet_jit_rng_pick as *const u8);
    builder.symbol("jet_jit_rng_shuffle", jet_jit_rng_shuffle as *const u8);
    builder.symbol(
        "jet_jit_rng_weighted_pick",
        jet_jit_rng_weighted_pick as *const u8,
    );
    builder.symbol("jet_jit_rng_sample", jet_jit_rng_sample as *const u8);
    builder.symbol("jet_jit_rng_bytes", jet_jit_rng_bytes as *const u8);
    builder.symbol("jet_jit_rng_split", jet_jit_rng_split as *const u8);
}

pub(crate) fn declare_random_host_fns(
    module: &mut cranelift_jit::JITModule,
) -> Result<RandomHostFns, String> {
    use cranelift_codegen::ir::{types, AbiParam, Signature};
    use cranelift_module::{Linkage, Module};

    let cc = module.target_config().default_call_conv;
    let mut import = |name: &str, sig: &Signature| -> Result<cranelift_module::FuncId, String> {
        module
            .declare_function(name, Linkage::Import, sig)
            .map_err(|e| e.to_string())
    };

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

    Ok(RandomHostFns {
        seed: import("jet_jit_random_seed", &sig_void_i64)?,
        bool_p: import("jet_jit_random_bool", &sig_f64_i8)?,
        float_range: import("jet_jit_random_float_range", &sig_f64_f64_f64)?,
        normal: import("jet_jit_random_normal", &sig_f64_f64_f64)?,
        exponential: import("jet_jit_random_exponential", &sig_f64)?,
        bytes: import("jet_jit_random_bytes", &sig_i64_i64)?,
        weighted_pick: import("jet_jit_random_weighted_pick", &sig_i64_i64_i64)?,
        sample: import("jet_jit_random_sample", &sig_i64_i64_i64)?,
        rng_new: import("jet_jit_rng_new", &sig_i64_i64)?,
        rng_int: import("jet_jit_rng_int", &sig_i64_i64_i64_i64)?,
        rng_float_range: import("jet_jit_rng_float_range", &sig_rng_fr)?,
        rng_bool: import("jet_jit_rng_bool", &sig_rng_bool_default)?,
        rng_bool_p: import("jet_jit_rng_bool_p", &sig_rng_bool)?,
        rng_pick: import("jet_jit_rng_pick", &sig_i64_i64_i64)?,
        rng_shuffle: import("jet_jit_rng_shuffle", &sig_void_i64_i64)?,
        rng_weighted_pick: import("jet_jit_rng_weighted_pick", &sig_i64_i64_i64_i64)?,
        rng_sample: import("jet_jit_rng_sample", &sig_i64_i64_i64_i64)?,
        rng_bytes: import("jet_jit_rng_bytes", &sig_i64_i64_i64)?,
        rng_split: import("jet_jit_rng_split", &sig_i64_i64)?,
    })
}
