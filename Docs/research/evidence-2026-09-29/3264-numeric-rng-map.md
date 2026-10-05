# #3264 — Exact numeric and simulation-RNG map (CORE-F040)

Date: 2026-09-29. Binary: `jet-debug-snapshot14` (sha256 `00b1e35e…d3fd`), source rev
`a8d8417c2`→`e9c708fa7` during the session. Author: Closer12. No compiler, Core or example change.

## Question

What exactly do Int, fixed widths, Float, Decimal, Fraction and seeded RNG do today about
rounding, overflow, NaN and conversions, on every tier? Which cells are missing or broken?

## Method

- Throwaway probes in `~/.cache/jet-dev/scratch/Closer12/num/`: `nm_int`, `nm_fixed`,
  `nm_fixed_checked`, `nm_float`, `nm_conversions`, `nm_decimal`, `nm_rng`, `u8_trap`, `frac_cells`.
- Existing witnesses were copied unchanged into scratch: `math/{fraction,fraction_exact,exact_rational_math,
  int_from_u64_extreme,random_audit,random_extremes,min_max_nan,math_audit,round_half_away}.jet`
  and `lowlevel/sized_integers.jet`.
- Every file ran through `~/.cache/jet-dev/scratch/Closer12/tiers.sh`, which runs
  `jet run` (Cranelift), `jet run --interpret`, and `jet build` followed by the built
  `.jet/build/<name>` binary. Outputs were compared with the repo golden when one exists, else with
  `jet run`. Raw outputs are in `~/.cache/jet-dev/scratch/Closer12/out/<name>/`.
- Every `jet run`/`--interpret` invocation in a scratch directory also prints
  `E2105 Record index update failed … record index path must stay under .jet/<artifact-kind>`
  to stderr after the program. This is ignored below. It is a separate defect (see list).

## Cell table (observed; every value is printed output, never inferred from a name)

| Cell | Input | run | interp | AOT | Policy observed |
|---|---|---|---|---|---|
| Int overflow | `9223372036854775807 + 1` | `9223372036854775808` | same | same | Int is exact and arbitrary precision; there is no overflow |
| Int `math.checked_add` | `(i64max, 1) ?? -1` | `9223372036854775808` | same | same | never `None` on Int; the name has no overflow meaning for Int |
| Int `math.saturating_add` | `(i64max, 1)` | `9223372036854775808` | same | same | no saturation; identical to `+` (Core/math/math.jet:252-256) |
| Int checked div | `checked_div(7, 0) ?? -1` | `-1` | same | same | zero divisor → `None` |
| `/` on Int | `7 / 2` | `3.5` | same | same | exact division (D-TYPE2-DEFAULT1) |
| Fixed-width default overflow | `U8{200} + U8{100}` in a fn | `before`, then `Stop [E3010] addition overflows`, exit **101** | same, exit **101** | same, exit **70** | traps. Exit code differs by tier (defect) |
| `wrapping(U8 200+100)` | | `44` | same | same | modular |
| `saturating(U8 200+100)` | | `255` | same | same | clamps |
| `U64.MAX.wrapping_add(1)` | | `0` | same | same | modular |
| `wrapping/saturating(I64 max + 1)` | | `-9223372036854775808` / `9223372036854775807` | same | same | modular / clamp |
| `U8.checked_add` | `U8{200}.checked_add(U8{100}) ?? U8{0}` | `0` | `0` | **rustc ICE** (`expected Result<u8, JetAbsent>, found Option<u8>`) | `None` on overflow; AOT broken |
| NaN equality | `NaN == NaN` | `false` | same | same | IEEE |
| NaN ordering | `NaN < 1`, `NaN > 1` | `false false` | same | same | IEEE unordered |
| `math.cmp` total order | `cmp(NaN,1)`, `cmp(1,NaN)`, `cmp(1,2)`, `cmp(NaN,NaN)` | `1 -1 -1 0` | same | same | total order, NaN sorts greatest (positive NaN) |
| Float div by zero | `1/0`, `-1/0`, `0/0` | `inf`, `-inf`, NaN | same | same | IEEE, no trap |
| Float sum | `Float{0.1}+Float{0.2}` | `0.30000000000000004` | same | same | binary64, shortest round-trip print |
| `math.round` | `2.5, -2.5, 0.5` | `3 -3 1` | same | same | half away from zero (round_half_away.jet also matches its golden on all 3 tiers) |
| Int→Float | `Float.from_int(9007199254740993)` | `9007199254740992.0` | same | same | silent round-to-nearest loss above 2^53 |
| Float→Int via `math.round` | `math.round(1.0e20)` | `9223372036854775807` | same | same | **silently saturates to i64 max** although the result type is exact Int (defect) |
| Decimal add | `decimal("0.1")+decimal("0.2")` | `0.3` | same | same | exact base-10 |
| Decimal round | `decimal("1.005").round()` | **ICE** `jet_decimal_round is not registered` | `1` | `1` | round to integer; half away (`2.5→3`, `-2.5→-3`) |
| Decimal div | `decimal("1").div(decimal("3"))` | ICE (same file) | `1/3` | `1/3` | exact: the quotient renders as a ratio, not a scaled decimal |
| Decimal scale | `decimal("1.10").mul(decimal("2.0"))` | ICE | `2.20` | `2.20` | scale kept from the operands |
| Decimal equality | `decimal("1.10") == decimal("1.1")` via `.equal` | ICE | `true` | `true` | numeric equality ignores trailing zeros |
| Fraction construct | `math.fraction(6,-8)`, `(1,0)`, `(1,3)` | **`None` for all three**, then ICE `JIT drop Fraction<>? value is not a result` | E0956 `Native Fraction route requires a Fraction carrier` | `-0.75`, `None`, `0.3333333333333333` | zero denominator → `None`; sign normalised; AOT only |
| `1 / 3 * 3 == 1` | | `true` | same | same | exact rational literal arithmetic |
| Seeded RNG replay | `random.seed(42); random.int(0,1000000)` ×2, then reseed and compare | **hangs** (300 s timeout, nothing after the header) | E0956 `MIR execution exhausted its fuel` | **rustc ICE** (`jet_rng_int` expects `&mut Rng`/`i64`, gets `&Rng`/`JetInt`) | not demonstrable on any tier |
| `Rng` split replay | `random.rng(99)`, `&r.split()` | not reached | not reached | not reached | unknown |
| floor div / remainders | `7 /% 2`, `-7 /% 2`, `-7 %% 2` | `3`, `-4`, `-1` | same | same | `/%` floors. `%%` is the truncated remainder (dividend's sign) and `%` is the floored modulo that pairs with `/%` (D-MODSEM1=A, `math/modulo.jet`) |

### Existing witnesses (same harness)

| Example | run | interp | AOT |
|---|---|---|---|
| `math/fraction.jet` | panic `a third is a ratio` (`math.fraction(1,3)` → None), exit 101 | E0956 Native Fraction route | rustc ICE (`jet_fraction_numerator` i64 vs JetInt) |
| `math/fraction_exact.jet` | = golden | = golden | = golden |
| `math/exact_rational_math.jet` | = golden | = golden | = golden |
| `math/int_from_u64_extreme.jet` | = golden | = golden | = golden |
| `math/random_audit.jet` | prints 5 lines then hangs at `random.sample` (timeout 124) | E0956 fuel exhausted | rustc ICE (`jet_rng_sample` `&mut Rng`) |
| `math/random_extremes.jet` | = golden | E0956 ``core.handle.rng.int()`` unsupported | rustc ICE (`jet_rng_int`) |
| `math/round_half_away.jet` | = golden | = golden | = golden |
| `math/min_max_nan.jet` | front-end E0112 `min wants Int … this is Float` (no golden exists) | same | same |
| `math/math_audit.jet` | ICE `checked Core call core.math.copy has no canonical TIR record` | same ICE | build fails |
| `lowlevel/sized_integers.jet` | = golden | E0956 ``core.math.is_infinite()`` unsupported | rustc ICE (`jet_u8_checked_add`) |

The card's named proof (`for f in fraction fraction_exact exact_rational_math random_audit; do jet run … | diff`)
fails on `fraction` and `random_audit` with both `jet run` and `--interpret`.

## Hardware vectors vs GPU kernels (criterion 2)

Read, not run: `Core/compute` (`core.compute` export row, CoreCallRows.jet:1908) is the device/tensor
surface. Its devices are `device_cpu/cuda/metal/vulkan/webgpu`, and it has `matmul`, `fft`, tensors and gradients.
Hardware SIMD is the separate `jet inspect accel` "vector proof and acceleration gate decisions"
surface. This map claims nothing about either tier's precision or speed; no vector or GPU cell was
exercised tonight.

## Missing types or operations (one gate each, for Pip to file)

1. **Int `saturating_*` / `checked_add|sub|mul|neg|abs` names.** They are observably identical to plain
   arithmetic or always `Val` on exact Int. Ballot: retire them, or restrict the names to fixed widths.
2. **Float→Int conversion that cannot silently saturate.** `math.round(1e20)` returns i64 max.
   The owner must choose between an exact result and a fallible `Int?`.
3. **Complex numbers.** D-NUMTYPE1=A ratified Complex, and `Core/math/math.jet:145` has
   `imag(n: Float) -> Float { 0.0 }`. No Complex cell could be exercised from source tonight
   [INFERENCE: carrier not located]. Needs an owner-scoped witness card.
4. **Decimal rounding mode and scale control.** Only `round/floor/ceil` to an integer were found.
   There is no `round(scale, mode)` or banker's mode. Gate if the finding's baseline needs it.

## Verdict

PASS on the investigation criteria. Criterion 1: policies are recorded per cell, and broken cells are
recorded as observed. Criterion 2: the vector/GPU distinction is stated from source, with no claim.
Criterion 3: the gates listed above. Criterion 4: every cell cites printed output. The card's Proof
diff loop fails for `fraction` and `random_audit`. Those failures, and the Decimal-round, RNG and
fixed-width `checked_add` tier failures, are filed as defects.
