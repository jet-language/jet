# Defined behavior: Jet against Odin, C, Zig and Rust

Date: 2026-10-30. Card #6323, criteria 1 and 2. Probes live in
`/mnt/jetscratch/lanes/MemDesign/probes/ub/` (written by `gen.mjs`), with
outputs in `probes/out/<name>.<tier>.{out,err,rc}`. They ran with
`/mnt/jetscratch/discovery/base/latest/jet` (bd83fe6361) on o0, o1 and o2.
Release binaries were built with `jet build --profile=release` and run
directly (`probes/ub/release.sh`). Helpers `r(n)` and `rf(x)` hide values
from compile-time folding.

## What "no undefined behavior" means elsewhere

Undefined behavior (UB) means the language gives the program no meaning at
all, so the optimizer may assume it never happens. Unspecified behavior picks
one of several allowed results. Platform-defined behavior differs by target.

| Operation | C | Zig | Rust (safe code) | Odin |
|---|---|---|---|---|
| Signed `+ - *` overflow | UB | Panic in Debug and ReleaseSafe; UB in ReleaseFast | Panic in debug; wraps in release | Wraps (two's complement) |
| Unsigned overflow | Wraps | As signed | As signed | Wraps |
| Shift by width or more | UB | Shift type is too narrow to hold it | Panic in debug; count masked in release | Result is 0 |
| Negative shift count | UB | Unsigned count type | Panic in debug; masked in release | Not allowed |
| `MIN / -1` | UB | Panic in safe modes; UB in ReleaseFast | Panic | Result is `MIN` |
| Integer divide by zero | UB | Panic in safe modes; UB in ReleaseFast | Panic | Panic |
| Float to int, out of range | UB | Panic in safe modes; UB in ReleaseFast | Saturates; NaN gives 0 | The overview does not say |
| Read of uninitialized memory | UB | UB | Not possible | Zero by default; `---` leaves it unset |
| Out-of-range index | UB | Panic in safe modes; UB in ReleaseFast | Panic | Panic; UB under `#no_bounds_check` |
| Data race | UB | UB | Not possible | Not defined |
| Float contraction (fused multiply-add) | Allowed | Off unless `@mulAdd` | Off | Allowed |

Sources:

- C11 6.5p5 and 6.5.7.
- The Zig 0.15 language reference, section "Illegal Behavior".
- The Rust Reference, "Behavior considered undefined".
- The Odin overview (`https://odin-lang.org/docs/overview/`), in a text copy
  saved on 2026-10-30. Lines 1402-1426 cover integer and float operators,
  line 1755 covers `---`, and lines 2176-2181 cover `#no_bounds_check`.

Odin's claim covers arithmetic. It does not make memory errors defined. Its
multi-pointer `[^]T` indexes with no bounds check (overview line 2898), and
`#no_bounds_check` and `-no-bounds-check` turn checks off. Ginger Bill
keeps `nil` and says Odin does not solve temporal safety
(`https://www.gingerbill.org/article/2026/01/02/was-it-really-a-billion-dollar-mistake`).
A Hacker News thread argues the same point
(`https://news.ycombinator.com/item?id=45113277`).

## Jet today, operation by operation

Every row printed the same output on o0, o1 and o2. "Stop" means a safe
runtime stop with a Jet report and exit status 70.

| Operation | Probe | Jet result | Class |
|---|---|---|---|
| `Int` overflow | `add_ovf`, `sub_ovf`, `mul_ovf`, `ct_add_ovf` | None: `Int` has no bound. `Int.MAX + 1` is 9223372036854775808, also at compile time. | Defined |
| `-Int.MIN`, `Int.MIN.abs()` | `neg_min`, `abs_min` | 9223372036854775808 | Defined |
| `Int.MIN / -1`, `Int.MIN % -1` | `min_div_neg1`, `min_mod_neg1` | 9223372036854775808 and 0 | Defined |
| `1 << 64`, `1 << 63` | `shl_64`, `shl_63` | 18446744073709551616, 9223372036854775808 | Defined |
| `-8 >> 70`, `-8 >> 1` | `shr_70`, `shr_neg_value` | -1 and -4 (arithmetic) | Defined |
| `1 << -1` | `shl_neg` | Stop E3010, "Invalid shift count" | Defined stop |
| Fixed-width `+ - *` overflow (`U8`, `I32`, `U64`) | `i32_add_ovf`, `u8_add_ovf`, `u8_sub_ovf`, `u64_sub_ovf` | Stop E3010, "This addition (subtraction) overflows the value's type" | Defined stop |
| `U8{r(300)}` | `narrow_u8` | Stop E3010, "Value doesn't fit in destination type" | Defined stop |
| `I32` overflow in a release build | `release.sh` | Stop E3010, the same | Defined stop |
| `I32{MIN} / I32{-1}` | `int_div` | Stop E3010, "This division overflows the value's type" | Defined stop |
| Wrapping and saturating on fixed widths | `wrap_fixed` | `I32.wrapping_add` gives -2147483648, `saturating_add` 2147483647, `checked_add` prints `null` | Defined |
| `U8 << 9` | `u8_shl_9` | Stop E3010, "Shifting left by 9 bits is out of range" | Defined stop |
| Divide or remainder by zero | `div_zero`, `mod_zero` | Stop E3010, "divided by zero", in every profile | Defined stop |
| `Int / Int` | `int_div`, `int_div2` | Exact: `-7 / 2` is -3.5; `6 / 3` is 2 | Defined |
| `/%` and `%` on `Int` | `int_div2`, `int_mod3` | Floored: `-7 /% 2` is -4, `-7 % 2` is 1, `7 % -2` is -1 | Defined |
| `/` and `%%` on fixed widths | `int_div2`, `int_mod3` | Truncated: `I32 -7 / 2` is -3, `-7 %% 2` is -1 | Defined |
| `Float` divide by zero, `0.0 / 0.0`, `-0.0` | `float_div_zero` | `inf`, `-inf`, `NaN`; `NaN == NaN` is false; `-0.0` prints `-0.0`; `0.0 == -0.0` is true | Defined (IEEE 754) |
| Float printing | `float_print` | Shortest round trip: `0.30000000000000004`, `1e21`, `5e-324` | Defined |
| Fused multiply-add | `fma_contract` | Never fused: `a * a - p` gives 0.0 | Defined |
| `xs.sum()` on `[Float]` | `sum_order` | A fixed eight-lane tree: 2.0 for `[1e16, 1, -1e16, 1, 1, 1]`; a plain loop gives 3.0 | Defined (D-FRED1 A) |
| Float to `Int` | `f2i_big` | `Int.from_float(1e20)` and `NaN` fail; `-3.7` gives -3 (`2026-10-30-type-conversions.md`) | Defined |
| `U8{U16{300}}` against `U8{300}` | SurfaceDesign `m6313` | Wraps to 44 against Stop E3010 | Defined, two meanings |
| List index out of range | `bounds_read` | Stop E3010, "the list has 3 items, so position 5 doesn't exist" | Defined stop |
| Slice out of range | `slice_range` | Stop E3001, "can't slice 3 items from 1 to 9" | Defined stop |
| Deep recursion | `stack_deep` | Stop E3012, "Stack overflow in `depth`" | Defined stop; the depth limit depends on the machine |
| Argument evaluation order | `arg_order` | Left to right | Defined |
| Map iteration | `map_order`, `map_order2` | Sorted by key: 12 scrambled `String` keys and 10 scrambled `Int` keys come out in ascending order. The spec does not state it. | Defined in practice |
| Uninitialized read on some path | `uninit_read` | E0420 while compiling | Refused |
| A task writes a captured variable | `race_mut` | The task gets its own copy: `inside 1`, then `after 0` | Defined, no race |
| `String` length and index | `string_index` | Characters: `"héllo".len()` is 5, `s[1..<2]` is `é` | Defined |
| Raw read through null, past the end, misaligned (`#Unsafe`) | `raw_null`, `raw_past_end`, `raw_misaligned` | Fault R0801, "raw `read` outside … storage", also in release builds | Defined fault |
| Use after arena reset (`#Unsafe`) | `Examples/features/memory/unsafe_sentries.jet` | Fault R0802, exit 70 | Defined fault |

Two message defects showed up. The misaligned read reports "outside …
storage", which names the wrong cause. `#Layout(c, packed)` fails E1105 with
"A tag width applies only to enums."

## Where Jet still has undefined or platform-dependent behavior

### Removed bounds checks

`--bounds=off` and `--bounds=report` keep the access after a failed check.
The comment at `Compiler/JetIR/Source/BuildRoutes.jet:59-61` says so: "the
access goes on". Both values are `check_removal` gates
(`Compiler/JetFoundation/Source/Registry/BuildOptions.jet:46`), and the build
prints "note: `--bounds=off` removes bounds checks from this package's code".
A package or organization policy can forbid the gate (D-GATE-LAW1,
`Compiler/JetCli/Source/Cli/Host.jet:986-992`).

Probe `bounds_off2` reads `[10, 20, 30]` at positions 3, 4, 7, -1 and
100000000 in a release build:

| Build | Output |
|---|---|
| `--profile=release --bounds=off` | `0`, `0`, `125057127039552`, `125057127039488`, then SIGSEGV at line 23 |
| `--bounds=report` | Each line is reported, then the same values, then SIGSEGV with exit 139 |

Positions 7 and -1 print heap addresses. Probe `bounds_off3` writes 99 at
position 5. It exits 0 and prints `[10, 20, 30]`, so the write landed in
memory the list does not own. This is UB in the C sense. Odin's
`-no-bounds-check` is the same.

The ratified per-block form, `#Unchecked(bounds)` inside `#Unsafe("reason")`
(D-JAI-EXPERT1 A), is not built: probe `unchecked_bounds` fails E0927,
"`#Unchecked` isn't a known applied rule".

### Raw pointers in release builds

D-MEM-EXPERT1 A says development builds check raw pointers, and the checks
"stay silent in release builds". D-RAWPTR2 A says dev mode and hardened
builds verify range, liveness and alignment. By that contract, a bad raw
read in a release build is UB. The build does more than the contract: release
binaries still stop with R0801. `--profile=hardened` fails E2104, "is not
built yet".

### Math library results differ by platform

`core.math` sends `sin`, `cos`, `exp`, `ln`, `pow` and the other elementary
functions to the platform C library
(`Compiler/JetRuntime/Source/CompilerNeededMath.jet:1-30`). C libraries round
these functions differently. Probe `probes/libm/d.c` computes 200,000 values
of each function with glibc and with musl and compares the bits:

| Function | Results that differ | Largest difference |
|---|---|---|
| `sin` | 6,328 | 1 ulp |
| `cos` | 6,096 | 1 ulp |
| `tan` | 7,069 | 1 ulp |
| `atan2` | 5,915 | 1 ulp |
| `exp` | 124 | 1 ulp |
| `pow` | 124 | 1 ulp |
| `log` | 12 | 1 ulp |
| `cbrt` | 95,062 | 3 ulps |

Jet's Linux binaries match musl on all six `sin` values the probe checked
(`ub/sin_bits.jet`), on o0, o1 and o2. A build for macOS or Windows would use
that system's library, so the same pure program can print different digits
[INFERENCE]. D-DET1 says a function with an explicit `-[]>` bound is
reproducible (`Docs/spec/syntax-decisions.md:3303`). D-CPU-DISPATCH1 A asks
for identical bits across machine-code copies. Platform math breaks both.

### Other items

- Recursion depth: the stop is defined, but where it happens depends on the
  machine unless `#MaxDepth(n)` is written (D-RESOURCE-LIMIT1 A).
- Fixed-width narrowing has two meanings: `U8{U16{300}}` wraps and `U8{300}`
  stops. D-CONVERT-TRAIT1, still open, covers it.
- Foreign calls: Jet cannot check what C code does. D-FFI-VOUCH1 A puts each
  unchecked foreign row in one `#Unsafe("reason")` block.
- `#Layout(overlay)` (D-JAI-LAYOUT1 A) is not built: E1105, "isn't a known
  layout variant". It allows plain-data fields only, so it reads bits, not
  invalid values.
- Map iteration comes out sorted by key, but the spec does not promise it.

## Jet against Odin

Jet already defines more than Odin in five places. `Int` never overflows.
Fixed-width overflow stops instead of wrapping silently, with
`wrapping_`, `saturating_` and `checked_` methods when wrapping is wanted.
Floats never fuse. Reads of unset variables are refused while compiling.
Captured variables cannot race.

The spellings differ: Jet's `%` is floored and `%%` truncated, and Odin's
are the other way round (overview line 1265).

Jet matches Odin in one weak spot: a build setting can remove bounds checks
and make an out-of-range access UB. Odin's overview says nothing about
elementary math functions, and Jet's results there differ by platform.

## Owner choices raised

- Ballot D-UB-BOUNDARY1: where a Jet program may have UB, which decides
  `bounds: off`, `bounds: report` and release raw-pointer checks.
- Ballot D-MATH-BITS1: whether `core.math` gives the same bits on every
  platform.
