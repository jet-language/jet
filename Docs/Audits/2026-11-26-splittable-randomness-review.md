# Design Review: Splittable Randomness (Proposal Item P25)

**Date:** 2026-11-26
**Scope:** Keyed, order-independent splittable randomness (`rng.split(key…)` and `random.rng(seed, key…)`) in `core.math.random`.
**Status:** PASS (zero open ballots; zero duplicate RNG surfaces introduced).

---

## 1. Problem Statement & Mechanism Under Review

Proposal item P25 introduces **keyed splittable randomness**: deriving child deterministic `RNG` streams purely from `(parent_seed, keys…)` so that concurrent tasks, reordered call sites, and interleaved draws on a parent generator always produce identical child streams across every compilation tier (`O0`, `O1`, `O2`, comptime, and Web).

Before this change, every split operation in Jet was **call-order-dependent**:
- `RNG.split(^self) -> RNG` in `Core/Math/Random.jet` advanced `self.state` by one SplitMix64 step, so swapping `a: r.split(); b: r.split()` or drawing from `r` beforehand changed both child streams.
- `core.math.random.split(seed_value: Int) -[Rand]> RNG` mixed one word from the process-global ambient `jet.runtime.random` stream, inheriting both the `Rand` effect and global call-order sensitivity.

---

## 2. Complete Inventory of Randomness & Seeding Sites Today

Every location in the repository that defines, routes, or wraps deterministic or ambient randomness:

| # | Path | Role | Verdict for P25 |
|---|---|---|---|
| 1 | `Core/Math/Random.jet` (`core.math.random`) | Canonical deterministic `RNG` (SplitMix64), pure `rng(seed)`, stateful `RNG.split()`, and ambient `Rand` xorshift helpers (`seed`, `split`, `int`, `float`, ...). | **Primary cutover target.** Extend `RNG` with immutable `root_seed: U64`, keyed `RNG.split(^self, keys: ...String)`, and pure `rng(seed_value: Int, keys: ...String)`. |
| 2 | `Compiler/Prelude/Core.rows` | Authoritative `core.math.random` export/ownership row (`RNG`, `rng`, `split`, ...). | **No duplicate exports needed.** Keyed splitting lives on the existing `RNG.split` and `rng` symbols. |
| 3 | `Compiler/JetFoundation/Source/CoreDeclarations/Calls.jet` (line 441) | Effect-row policy: `"core.math.random" -> if member == "rng" -> None else -> Val("Rand")`. | **Preserved cleanly.** `random.rng(seed, keys...)` stays effect-free (`None`); ambient `random.split(seed)` keeps `Rand` and is not overloaded with a pure path that would contradict its `Rand` effect row. |
| 4 | `Compiler/JetFoundation/Source/Registry/CoreModules.jet` | Bare nominal `RNG` mapping (`CORE_BARE_SOURCE_MODULES`) and `RNG.bool(p)` single-arity route. | **No compiler change required.** Standard variadic `keys: ...String` needs no special Sema route. |
| 5 | `Compiler/JetRuntime/Source/Public/Random.jet` (`jet.runtime.random`) | Host-backed ambient stream state (`rng_state`, `rng_try_update`, `seed`). | **Untouched.** Keyed splitting is 100% pure Jet arithmetic with zero host/runtime calls. |
| 6 | `Core/Testing/Testing.jet` (`fake_rng`, `Fake`, `HistoryRNG`) | `fake_rng(seed)` delegates directly to `core.math.random.rng(seed)`. `Fake` delegates to `core.math.random.__seeded_int`. `HistoryRNG` (line 725) wraps a `Shared<HistoryRNGState>` SplitMix64 word exclusively for stateful `histories(...)` campaign replay. | **`fake_rng` inherits keyed `split(keys...)` automatically.** `HistoryRNG` is a `Shared`-locked mutable campaign driver rather than a value-owned splittable RNG; it is left untouched and explicitly noted as a non-splittable testing-internal driver. |
| 7 | `Core/Crypto/Random.jet` (`core.crypto.random`) | Fail-closed OS CSPRNG (`Rand` / security entropy). | **Explicitly excluded.** Cryptographic randomness must never be deterministically split or merged with PRNG state. |
| 8 | `Core/Math/Stats.jet` | `kde_random` ambient `Rand` consumer. | **Untouched.** |
| 9 | `Tools/Editors/jet.tmGrammar` (and TreeSitter / VSCode / Zed) | Editor syntax grammars. | **Verified sufficient.** `rng.split("world", "chunk")` and `random.rng(seed, "worker", "0")` use existing method-call, variadic, and string-literal grammar rules; no new keyword or token is introduced. |

---

## 3. Cutover Plan, Copies Avoided, and Adversarial Design Checks

### 3.1 Single Canonical Mechanism (No Duplicate Surfaces)
- **No `core.random` duplicate module:** Jet's established module path is `core.math.random` (`Docs/Spec/spec.md`, `Docs/Spec/stdlib-api-laws.md`). P25 extends `core.math.random.RNG` and `core.math.random.rng` directly rather than creating a second module or a second RNG struct.
- **No duplicate `KeyedRNG` / `SplitRNG` type:** Every `RNG` carries `state: U64` (current draw position) and `root_seed: U64` (immutable origin seed for this stream). Drawing advances `state` only; keyed splitting reads `root_seed` only.
- **Shared finalizer:** `mix64` (Stafford variant 13 SplitMix64 finalizer) is factored out once in `Core/Math/Random.jet` and shared by both `seeded_next` and `split_key_step` so sequential draws keep bit-for-bit identical legacy outputs while keyed splits inherit full 64-bit avalanche.

### 3.2 Attack 1: Prior Draws Mutating Parent State
- **Failure mode attacked:** If keyed `split(keys...)` hashed `self.state`, then calling `parent.int(1, 10)` before `parent.split("enemy")` would alter the `"enemy"` child stream, breaking order-independence.
- **Resolution:** `RNG` stores `root_seed: U64` separately from `state: U64`. Keyed `split(keys...)` reads `self.root_seed`, never reads or writes `self.state`, and initializes the child's `state` and `root_seed` both to the derived seed. Even drawing from an intermediate child (`^world: root.split("world"); world.int(1, 10)`) never affects `world.split("chunk")`.

### 3.3 Attack 2: Concatenation & Prefix Collisions
- **Failure mode attacked:** Naive string concatenation makes `split("a", "bc") == split("ab", "c") == split("abc")` and `split("") == parent`.
- **Resolution:** Each key step `split_key_step(seed, key)` domain-separates with the golden-ratio constant (`0x9E3779B97F4A7C15`), mixes `key.bytes().len()` before absorbing 8-byte little-endian lanes, and finishes with an independent odd-constant Stafford-13 finalizer (`0x6A09E667F3BCC909`).
- **Algebraic consequence:** Folding `split_key_step` left-to-right over `keys` guarantees **path associativity** (`r.split("a", "b")` == `r.split("a").split("b")` == `random.rng(seed, "a", "b")`) while preserving strict domain separation (`"a","bc"` != `"ab","c"` != `"abc"`, `""` != parent, `"a",""` != `"a"` != `"","a"`).

### 3.4 Attack 3: Key Type Ergonomics & Compiler Surface
- **Finding from probing the live compiler:** Heterogeneous trait-bounded variadic methods (`keys: ...SplitKey`) currently hit an unlowered IR fault (`ir.build unlowered: a value of String where SplitKey is expected`), and a generic variadic (`fn<T: SplitKey>(^self, keys: ...T)`) fails type inference on zero-argument `r.split()`.
- **Resolution:** `keys: ...String` compiles cleanly across `O0`, `O1`, `O2`, comptime, and Web with zero compiler changes, preserves zero-argument `r.split()` backwards compatibility for all existing callers and known-answer tests, and composes idiomatically with Jet string interpolation (`r.split("chunk", "{x}", "{y}")`). If heterogeneous trait variadic lowering is completed in a future compiler card, widening `String` keys to a trait can be done cleanly without breaking existing `String` call sites.

### 3.5 Attack 4: Parallel Task Ergonomics with Non-Copy `RNG`
- **Failure mode attacked:** `RNG` is intentionally non-implicitly-copyable (`core_math_rng_is_not_implicitly_copyable`), and `RNG.split` keeps `^self` to preserve backwards compatibility with stateful zero-argument `r.split()`. Two concurrent `task` closures therefore cannot both borrow `^parent` simultaneously.
- **Resolution:** Two clean patterns work without sacrificing ownership safety or adding compiler overloads:
  1. Pre-split children on the parent (`^c0: parent.split("worker", "0")`, `^c1: parent.split("worker", "1")`) and move each child into its task.
  2. Derive directly from the immutable `seed: Int` inside any task via `random.rng(seed, "worker", "0")`, which is pure (no `Rand` effect) and produces the bit-for-bit identical stream to `random.rng(seed).split("worker", "0")`.

### 3.6 Attack 5: Tier Parity (I9)
- **Check:** `mix64`, `split_key_step`, and `derive_split_seed` use only exact `U64` `wrapping` arithmetic, bit shifts, XOR, and `String.bytes()`. There are zero floating-point operations, zero host/libc calls, and zero platform-endian assumptions. Output is bit-for-bit identical across `O0`, `O1`, `O2`, comptime, and Web.

---

## 4. Verification & Cutover Checklist

1. `Core/Math/Random.jet`: `mix64`, `split_key_step`, `derive_split_seed`, `RNG.root_seed`, `RNG.split(^self, keys: ...String)`, `random.rng(seed_value: Int, keys: ...String)`, plus unit `#Test` blocks proving order-independence, draw-independence, associativity, domain separation, and 64-key Hamming avalanche diffusion.
2. `Tests/Cases/CoreMathNumeric/core_math_splittable_rng_keyed_streams_match_across_orders_tasks_and_tiers.case`: multi-order, concurrent `task.all`, and multi-tier (`O0`/`O1`/`O2`) verification.
3. `Examples/Math/SplittableRandom.jet` + `.out` + `Examples/Math/README.md`: executable golden example verified across `O0`/`O1`/`O2`.
4. `Docs/Spec/spec.md` and `Docs/Spec/Reference/core-library.md`: normative specification and Core library reference updates, plus editor grammar verification (`Tools/Editors/jet.tmGrammar`).
5. `Tests/Guards/CoreDeclarations` and `Tools/Agent/AgentDocFlags.jet -- --policy-only`: all guards clean.

**Audit verdict:** PASS.
