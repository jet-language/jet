//! Ambient random evaluator adapters.

// D-DET1 / I9: ambient random behavior is the runtime Prelude kernel. These
// wrappers only marshal CtValue containers around that kernel.
pub(super) mod ambient_random_kernel {
    pub(crate) mod jet_std {
        #[derive(Clone)]
        pub(crate) struct Rng {
            pub(crate) state: u64,
        }
    }

    include!("../../../../../jet-codegen/src/Prelude/CoreLib/Top/MathRandomFns.rs");

    pub(crate) fn seed(seed: i64) {
        jet_std_random_seed(seed);
    }

    pub(crate) fn int(low: i64, high: i64) -> i64 {
        jet_std_random_int(low, high)
    }

    pub(crate) fn float() -> f64 {
        jet_std_random_float()
    }

    pub(crate) fn split(seed: i64) -> u64 {
        jet_std_random_split(seed).state
    }

    pub(crate) fn float_range(low: f64, high: f64) -> f64 {
        jet_std_random_float_range(low, high)
    }

    pub(crate) fn bool_p(p: f64) -> bool {
        jet_std_random_bool(p)
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

    pub(crate) fn pick<T: Clone>(items: &Vec<T>) -> Option<T> {
        jet_std_random_pick(items)
    }

    pub(crate) fn weighted_pick<T: Clone>(items: &Vec<T>, weights: &Vec<f64>) -> Option<T> {
        jet_std_random_weighted_pick(items, weights)
    }

    pub(crate) fn sample<T: Clone>(items: &Vec<T>, count: i64) -> Vec<T> {
        jet_std_random_sample(items, count)
    }

    pub(crate) fn shuffle<T>(items: &mut Vec<T>) {
        jet_std_random_shuffle(items);
    }
}
