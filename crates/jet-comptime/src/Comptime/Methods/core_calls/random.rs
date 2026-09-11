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
    type WorldRngNext = fn() -> Option<u64>;
    type WorldRngSeed = fn(i64) -> bool;

    thread_local! {
        static WORLD_RNG_NEXT: std::cell::Cell<Option<WorldRngNext>> =
            const { std::cell::Cell::new(None) };
        static WORLD_RNG_SEED: std::cell::Cell<Option<WorldRngSeed>> =
            const { std::cell::Cell::new(None) };
    }

    struct WorldRngProviderGuard {
        previous_next: Option<WorldRngNext>,
        previous_seed: Option<WorldRngSeed>,
    }

    impl Drop for WorldRngProviderGuard {
        fn drop(&mut self) {
            WORLD_RNG_NEXT.with(|provider| provider.set(self.previous_next));
            WORLD_RNG_SEED.with(|provider| provider.set(self.previous_seed));
        }
    }

    pub(crate) fn with_world_rng_provider<T>(
        next: WorldRngNext,
        seed: WorldRngSeed,
        callback: impl FnOnce() -> T,
    ) -> T {
        let previous_next = WORLD_RNG_NEXT.with(|provider| provider.replace(Some(next)));
        let previous_seed = WORLD_RNG_SEED.with(|provider| provider.replace(Some(seed)));
        let _guard = WorldRngProviderGuard {
            previous_next,
            previous_seed,
        };
        callback()
    }

    fn world_rng_next() -> Option<u64> {
        WORLD_RNG_NEXT.with(|provider| provider.get().and_then(|provider| provider()))
    }

    fn world_rng_seed(seed: i64) -> bool {
        WORLD_RNG_SEED.with(|provider| provider.get().is_some_and(|provider| provider(seed)))
    }


    // The comptime adapter has no direct dependency on jet-codegen's
    // Scheduler. Its scoped provider hook carries the same active-world
    // lookup across that crate boundary; absent scope keeps the host stream.
    fn jet_scheduler_world_rng_next() -> Option<u64> {
        world_rng_next()
    }
    fn jet_scheduler_world_rng_seed(seed: i64) -> bool {
        world_rng_seed(seed)
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
