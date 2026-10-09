//! Ambient random evaluator adapters.

// D-DET1 / I9: ambient random behavior is the runtime Prelude kernel. These
// wrappers only marshal CtValue containers around that kernel.
pub(super) mod ambient_random_kernel {
    type WorldRngSeed = fn(i64) -> bool;
    type WorldRngState = fn() -> Option<u64>;
    type WorldRngTryUpdate = fn(u64, u64) -> Option<bool>;

    thread_local! {
        static WORLD_RNG_SEED: std::cell::Cell<Option<WorldRngSeed>> =
            const { std::cell::Cell::new(None) };
        static WORLD_RNG_STATE: std::cell::Cell<Option<WorldRngState>> =
            const { std::cell::Cell::new(None) };
        static WORLD_RNG_TRY_UPDATE: std::cell::Cell<Option<WorldRngTryUpdate>> =
            const { std::cell::Cell::new(None) };
    }

    struct WorldRngProviderGuard {
        previous_seed: Option<WorldRngSeed>,
        previous_state: Option<WorldRngState>,
        previous_try_update: Option<WorldRngTryUpdate>,
    }

    impl Drop for WorldRngProviderGuard {
        fn drop(&mut self) {
            WORLD_RNG_SEED.with(|provider| provider.set(self.previous_seed));
            WORLD_RNG_STATE.with(|provider| provider.set(self.previous_state));
            WORLD_RNG_TRY_UPDATE.with(|provider| provider.set(self.previous_try_update));
        }
    }

    pub(crate) fn with_world_rng_provider<T>(
        seed: WorldRngSeed,
        state: WorldRngState,
        try_update: WorldRngTryUpdate,
        callback: impl FnOnce() -> T,
    ) -> T {
        let previous_seed = WORLD_RNG_SEED.with(|provider| provider.replace(Some(seed)));
        let previous_state = WORLD_RNG_STATE.with(|provider| provider.replace(Some(state)));
        let previous_try_update = WORLD_RNG_TRY_UPDATE.with(|provider| provider.replace(Some(try_update)));
        let _guard = WorldRngProviderGuard {
            previous_seed,
            previous_state,
            previous_try_update,
        };
        callback()
    }

    fn world_rng_seed(seed: i64) -> bool {
        WORLD_RNG_SEED.with(|provider| provider.get().is_some_and(|provider| provider(seed)))
    }


    // The comptime adapter has no direct dependency on jet-codegen's
    // Scheduler. Its scoped provider hook carries the same active-world
    // lookup across that crate boundary; absent scope keeps the host stream.
    fn jet_scheduler_world_rng_seed(seed: i64) -> bool {
        world_rng_seed(seed)
    }
    fn jet_scheduler_world_rng_state() -> Option<u64> {
        WORLD_RNG_STATE.with(|provider| provider.get().and_then(|provider| provider()))
    }

    fn jet_scheduler_world_rng_try_update(expected: u64, replacement: u64) -> Option<bool> {
        WORLD_RNG_TRY_UPDATE.with(|provider| provider.get().and_then(|provider| provider(expected, replacement)))
    }

    pub(crate) fn state() -> u64 { jet_random_state_raw() }
    pub(crate) fn try_update(expected: u64, replacement: u64) -> bool {
        jet_random_try_update_raw(expected, replacement)
    }
    include!("../../../../../jet-codegen/src/Prelude/CoreLib/Top/MathRandomFns.rs");

    pub(crate) fn seed(seed: i64) {
        jet_std_random_seed(seed);
    }

}
