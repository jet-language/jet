// D-DET-CAPAPI: the deterministic seeded-Rng value kernel.
//
// This file has no Jet value types.  Runtime tiers own the handle or CtValue
// marshalling and call these functions for the one SplitMix64 stream.

pub(crate) fn jet_seeded_rng_next(state: &mut u64) -> u64 {
    *state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut z = *state;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

pub(crate) fn jet_seeded_rng_int(state: &mut u64, low: i64, high: i64) -> i64 {
    if high <= low {
        return low;
    }
    low + (jet_seeded_rng_next(state) % (high - low + 1) as u64) as i64
}

pub(crate) fn jet_seeded_rng_float(state: &mut u64) -> f64 {
    (jet_seeded_rng_next(state) >> 11) as f64 / (1u64 << 53) as f64
}

pub(crate) fn jet_seeded_rng_float_open(state: &mut u64) -> f64 {
    jet_seeded_rng_float(state).max(f64::MIN_POSITIVE)
}

pub(crate) fn jet_seeded_rng_float_range(state: &mut u64, low: f64, high: f64) -> f64 {
    if !(high > low) {
        return low;
    }
    low + (high - low) * jet_seeded_rng_float(state)
}

pub(crate) fn jet_seeded_rng_bool(state: &mut u64) -> bool {
    jet_seeded_rng_next(state) & 1 == 1
}

pub(crate) fn jet_seeded_rng_bool_p(state: &mut u64, probability: f64) -> bool {
    if probability <= 0.0 || probability.is_nan() {
        false
    } else if probability >= 1.0 {
        true
    } else {
        jet_seeded_rng_float(state) < probability
    }
}

pub(crate) fn jet_seeded_rng_normal(state: &mut u64, mean: f64, stddev: f64) -> f64 {
    let u1 = jet_seeded_rng_float_open(state);
    let u2 = jet_seeded_rng_float(state);
    let z0 = (-2.0 * u1.ln()).sqrt() * (std::f64::consts::TAU * u2).cos();
    mean + z0 * stddev.max(0.0)
}

pub(crate) fn jet_seeded_rng_exponential(state: &mut u64, lambda: f64) -> f64 {
    if lambda <= 0.0 || lambda.is_nan() {
        0.0
    } else {
        -jet_seeded_rng_float_open(state).ln() / lambda
    }
}

pub(crate) fn jet_seeded_rng_bytes(state: &mut u64, count: i64) -> Vec<u8> {
    let mut out = Vec::with_capacity(count.max(0) as usize);
    for _ in 0..count.max(0) {
        out.push(jet_seeded_rng_next(state) as u8);
    }
    out
}

pub(crate) fn jet_seeded_rng_split(state: &mut u64) -> u64 {
    jet_seeded_rng_next(state)
}
