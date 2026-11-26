# Adversarial Design Review: Splittable Randomness (`#7022`, Proposal Item P25)

- **Date:** 2026-11-26
- **Card:** #7022 (greenfield proposal item P25)
- **Scope:** Keyed, order-independent splittable randomness (`rng.split(key…)` and `random.rng(seed, key…)`) in `core.math.random`, elimination of duplicate SplitMix64 implementations and competing `split` meanings, integer and string keys, and multi-tier parity (`O0`, `O1`, `O2`, comptime, and Web).
- **Verdict:** **FAILS AS ORIGINALLY SHIPPED; PASSES ONLY AFTER ALL 4 BINDING CUTOVER FIXES BELOW LAND AND PASS FULL SUITE.**
  The SplitMix64 keyed derivation (`root_seed` separated from `state`, length-prefixed mixing, path associativity) is mathematically sound, but adversarial review of the initial build uncovered **four real gaps**: three competing meanings of `split` left alive, a duplicate SplitMix64 copy left in `Core/Testing/Testing.jet:742-750`, integer keys (`chunk.x, chunk.y`) wrongly deferred to a nonexistent compiler card despite `...(Int | String)` working today, and missing Web-tier test coverage.

---

## 1. Strongest Existing Solutions (with Primary Sources)

1. **JAX PRNG Keys (`jax.random.key`, `split`, `fold_in`)** (`https://jax.readthedocs.io/en/latest/jep/263-prng.html`, `https://jax.readthedocs.io/en/latest/random-numbers.html`):
   Pure functional counter-based PRNG (Threefry/Philox) where `fold_in(key, int_data)` deterministically folds integer indices into a key without mutating the parent, making parallel JIT/device execution completely independent of evaluation order. Strongest peer for keyed/order-independent derivation; JAX explicitly supports integer keys (`fold_in(key, step)`).
2. **Java `java.util.SplittableRandom`** (`https://docs.oracle.com/en/java/javase/21/docs/api/java.base/java/util/SplittableRandom.html`; Steele, Lea, Flood, OOPSLA 2014 *Fast Splittable Pseudorandom Number Generators*):
   Introduced the SplitMix64 (Stafford variant 13) construction used in Jet. Weakness: Java's zero-argument `split()` advances the parent's internal state, so child streams depend on call order and break if two branches reorder their splits. Jet's keyed `split(key…)` fixes Java's order-dependence while keeping SplitMix64's speed and statistical quality.
3. **NumPy `numpy.random.SeedSequence.spawn`** (`https://numpy.org/doc/stable/reference/random/bit_generators/index.html#seeding-and-entropy`):
   Uses a hierarchical `spawn_key` tuple of integers hashed through a mixing pool so each child's stream is determined by `(entropy, spawn_key)` path rather than mutable generator draws. Strongest peer for hierarchical path-keyed seeding (`(parent, k1, k2)`), though limited to integer spawn keys.
4. **Haskell `splitmix` (`System.Random.SplitMix`)** (`https://hackage.haskell.org/package/splitmix`):
   Pure functional `splitSMGen :: SMGen -> (SMGen, SMGen)` returns a pair of independent generators. Pure, but positional (binary tree path depends on left/right traversal order) rather than named/integer-keyed.

---

## 2. Adversarial Attack on Every Claim

### Attack 1: Three Competing Meanings of `split` Remain (Violates I8)
- **Claim:** One canonical `split` mechanism exists for deriving child streams.
- **Attack:** **FAILED in the original build.**
  `Core/Math/Random.jet` kept **three** different `split` meanings simultaneously:
  1. `RNG.split(^self, keys: ...String)` with >= 1 key: pure, order-independent derivation from `self.root_seed` (did not mutate `self.state`, yet forced `^self` because of meaning #2).
  2. `RNG.split(^self)` with 0 keys (`Random.jet:167-170`): mutated `self.state` sequentially (order-dependent!).
  3. Top-level `core.math.random.split(seed_value: Int) -[Rand]> RNG` (`Random.jet:305-312`): mutated the ambient `Rand` stream and mixed it with `seed_value`.
- **Root fix (binding):** Consolidate onto **one** meaning for `split`:
  - `RNG.split(self, key: Int | String, rest: ...(Int | String)) -> RNG Never!` takes **immutable `self`** and 1+ keys (`Int | String`), deriving purely from `self.root_seed` without mutating `self.state`.
  - Delete top-level ambient `random.split(seed_value: Int)` from `Core/Math/Random.jet` and `Compiler/Prelude/Core.rows`, delete `Tests/Conformance/Corpus/Core/Math/Random/split.jet`, and migrate its 4 test callers + the 6 zero-argument `.split()` callers to explicit keyed `split(...)` / `rng(seed, ...)`.

### Attack 2: Duplicate SplitMix64 Implementation Left Alive
- **Claim:** SplitMix64 mixing has one source of truth in `Core/Math/Random.jet`.
- **Attack:** **FAILED in the original build.**
  `Core/Testing/Testing.jet:742-750` (`history_rng_draw`) duplicates the exact SplitMix64 step and Stafford-13 constants (`11400714819323198485`, `13787848793156543929`, `10723151780598845931`) even though `core.testing` already depends on `core.math.random` in `Compiler/Prelude/Core.rows:224`.
- **Root fix (binding):** Delete the duplicate SplitMix64 arithmetic in `Core/Testing/Testing.jet:742-750` and delegate `history_rng_draw` to `core.math.random.seeded_next(guard.value.word)`.

### Attack 3: Integer Keys (`chunk.x, chunk.y`) Wrongly Deferred
- **Claim:** Integer keys required a future compiler card for trait-bounded variadics.
- **Attack:** **FAILED in the original build.**
  Probing `keys: ...(Int | String)` on the live compiler proved union variadics already compile and execute identically across `O0`, `O2`, and `--target web` (`node .jet/build/app.js`) with zero compiler changes! Deferring integer keys to a nonexistent card was unnecessary and violated P25's core `rng.split(chunk.x, chunk.y)` use case.
- **Root fix (binding):** Support `Int | String` directly in `RNG.split(self, key: Int | String, rest: ...(Int | String))` and `rng(seed_value: Int, keys: ...(Int | String))` with explicit type-tag domain separation (`U64{1}` for `String`, `U64{2}` for `Int` so `split("42")` != `split(42)`), preserving path associativity (`r.split("chunk", x, y) == r.split("chunk").split(x).split(y) == random.rng(seed, "chunk", x, y)`).

### Attack 4: Missing Web-Tier Coverage (Violates I9)
- **Claim:** Keyed splittable randomness is verified on every applicable tier.
- **Attack:** **FAILED in the original build.**
  `Tests/Cases/CoreMathNumeric/core_math_splittable_rng_keyed_streams_match_across_orders_tasks_and_tiers.case` tested only `O0`, `O1`, and `O2`, omitting `--target web`.
- **Root fix (binding):** Add `jet build --target web` + `node .jet/build/app.js` steps to `core_math_splittable_rng_keyed_streams_match_across_orders_tasks_and_tiers.case` asserting identical output to `O0`/`O1`/`O2`.

### Attack 5: Cryptographic Boundary, Draw Independence, and Collision Resistance
- `core.crypto.random` stays strictly fail-closed OS CSPRNG (never deterministically split). **Holds.**
- `RNG` stores `root_seed: U64` separately from `state: U64`; draws advance `state` only, while `split` reads `root_seed` only. **Holds.**
- Type-tagged, length-prefixed SplitMix64 mixing prevents `"a","bc" == "ab","c"`, `"" == parent`, and `"42" == 42`. **Holds.**

---

## 3. Binding Cutover List

Every item below MUST land and pass before #7022 closes:

1. **Unify `split` & add `Int | String` keys (`Core/Math/Random.jet`, `Compiler/Prelude/Core.rows`):**
   - `RNG.split(self, key: Int | String, rest: ...(Int | String)) -> RNG Never!` (immutable `self`, type-tagged `Int` and `String` key steps, zero mutable fallback).
   - `rng(seed_value: Int, keys: ...(Int | String)) -> RNG Never!`.
   - Delete top-level ambient `split: fn(seed_value: Int) -[Rand]> RNG Never!` and remove `split` from `core.math.random` in `Compiler/Prelude/Core.rows`.
   - Delete `Tests/Conformance/Corpus/Core/Math/Random/split.jet`.
2. **Delete duplicate SplitMix64 (`Core/Testing/Testing.jet`):**
   - Replace `history_rng_draw` inline SplitMix64 math with `random.seeded_next(guard.value.word)` (export `seeded_next` or keep internal Core seam in `Core/Math/Random.jet` / `Core.rows`).
3. **Migrate all callers (`random.split(...)` and zero-arg `.split()`):**
   - `Tests/Run/CoreMathNumeric/CoreMathCalls.jet`
   - `Tests/Cases/CoreLibPartsEncodingSurface/NondeterministicCoreMatrixMatchesAllExecutionTiers.fixture/run.jet`
   - `Tests/Cases/CoreLibPartsEncodingSurface/RandomDistributionSurfaceIsDeterministic.fixture/run.jet`
   - `Tests/Cases/Repl/ReplCoreRandomWidenedDrawsDispatch.fixture/input9.txt`
   - `Examples/Math/RandomAudit.jet` + `.out` & `Tests/Fuzz/Sema/Differential/ex_math_random_audit.jet`
   - `Tests/Cases/CoreMathNumeric/CoreMathCallerRNGCoinProbabilityAndGenericDrawsMatchAcrossTiers/Input/run.jet`
   - `Tests/Cases/ComptimeCorePureParity/RustcBackedSeededRNGMethodsMatchAllExecutionTiersExactly/Input/{o1,o2}/run.jet`
4. **Update example, tests, web tier, and docs:**
   - `Examples/Math/SplittableRandom.jet` + `.out` (demonstrate both string and integer keys `chunk.x, chunk.y` and immutable `root.split(...)`)
   - `Tests/Cases/CoreMathNumeric/core_math_splittable_rng_keyed_streams_match_across_orders_tasks_and_tiers.case` + fixture (add integer keys + `--target web` + `node .jet/build/app.js` parity)
   - `Docs/Spec/spec.md`, `Docs/Spec/Reference/core-library.md`
5. **Full suite verification:**
   - `Core/Math/Random.jet`, `Core/Testing/Testing.jet`, `CoreDeclarations`, `SplittableRandom` golden (`O0`/`O1`/`O2`), `core_math_splittable_rng_keyed_streams_match_across_orders_tasks_and_tiers` (`O0`/`O1`/`O2`/Web), and full suite.
