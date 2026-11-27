# Crossed argument names design review (`#7020`, P23)

Date: 2026-11-26

## 1. Proposal and context

Proposal item P23 (Greenfield Language Proposal, 2026-11-24; redesigned in C49 of the 2026-11-24 review) specifies:

> Passing a variable named like a *different* parameter of the same type is an error: `copy(dst, src)` into `fn(src, dst)`.
> Why: catches a common swapped-argument bug at no cost to correct code.

An earlier draft required labels on every adjacent same-typed parameter pair, which failed immediately on `pow(x, 2)`, `atan2(y, x)`, and `max(a, b)`. Narrowing to crossed names avoids penalizing ordinary positional calls and leaves intentional label enforcement to Jet's existing `*` label-only zone (`E0769`) and `L2401` (`Jet.PositionalBoolParameter`).

This review attacks the narrowed rule against Jet's actual call-binding pipeline and measures both the naive single-argument interpretation and the refined cycle interpretation across more than 500,000 lines of real Jet code (`Core/`, `Compiler/`, `Compiler/JetRuntime/`, `Jetpack/`, `Tests/Guards/`, `Tools/Agent/`, and `Examples/`).

## 2. Existing call-binding architecture in `Compiler/JetSema`

Every multi-argument positional call in Jet passes through two canonical layers:

1. **Shape binding (`Compiler/JetSema/Source/Sema/Calls/Binder.jet`)**:
   - `sema_call_bind_args` maps written positional and labeled arguments onto declaration slots (`SemaCallParam`), enforcing `E0764`–`E0769`, `E0104`, and `E1312`.
   - `SemaCallParam` records `label` (the public call label, empty for unnamed function-type parameters), `name`, `zone` (`PositionalOnly` before `/`, `Either`, or `LabelOnly` after `*`), `ty`, `default`, `default_ref`, `convention`, and `variadic`.
   - Call-site binding runs *before* generic type inference settles parameter types (`Calls/Direct.jet`, `Calls/Methods.jet`), so `Binder.jet` cannot judge type equality on generic calls such as `pair_up<T>(first: T, second: T)`.
2. **Bound argument typechecking (`Compiler/JetSema/Source/Sema/Calls/Checking.jet`)**:
   - `sema_call_typecheck_bound_arguments` is the single shared funnel reached by direct calls (`Calls/Direct.jet`), generic calls after inference (`Calls/Direct.jet`, `Calls/Methods.jet`), methods (`Calls/Methods.jet`), static methods (`Calls/Static.jet`), function values (`Calls/Values.jet`), Core module calls (`Calls/Core.jet`), callable policies (`CallablePolicies.jet`), and handle methods (`Plugin.jet`, `Reflection.jet`, `EventHandles.jet`, `ReactiveHandles.jet`, `CompilerServices.jet`).
   - By line 934 (`if !complete -> return None`), every argument has typechecked cleanly, every generic parameter has been substituted, `slots[i]` holds the declaration parameter slot, `required_types[i]` holds the resolved parameter type, `binding.sources[i]` distinguishes `.Written(_)` from `.Default(_)`, and `args[i].label` preserves whether the caller wrote an explicit label.
   - Struct literals (`Type{a: x, b: y}`) and multi-field enum variants (`Enum.Case{a: x, b: y}`) always require field labels in `Expressions/Constructors.jet`, so they cannot suffer positional crossing.
   - Existing parameter-name diagnostics live in `Compiler/JetSema/Source/Sema/FunctionHeaders.jet` (`L2401` `Jet.PositionalBoolParameter` for public positional `Bool` parameters) and `Calls/Binder.jet` (`E0764`–`E0772`). Code **`E0773`** is unused right inside the call-binding block (`E0764`–`E0772`).

**Home verdict:** `sema_call_typecheck_bound_arguments` in `Compiler/JetSema/Source/Sema/Calls/Checking.jet` (right after `if !complete -> return None` and `sema_call_crypto_misuse`) is the one sound, zero-duplication home.

## 3. Hostile attacks on the rule

### Attack 1: Naive one-way name matching has a ~90% false-positive rate on real Jet code

**Attack.** If any single unlabeled positional argument `x` bound to parameter `p` errors whenever *some* other same-typed parameter is named `x`, ordinary code with generic local variable names (`name`, `bits`, `start`, `value`, `text`, `number`, `c`, `a`, `b`) explodes whenever the other parameter receives a literal or compound expression.

**Empirical measurement (V1 one-way rule, even with `rect(width, width)` duplication already exempt):**

| Tree | Total V1 hits | One-way hits | Mutual-cross hits | Representative V1 false positives |
| --- | ---: | ---: | ---: | --- |
| `Core/` (~200k LOC, all bodies) | 82 | 64 | 18 | `set_header(client, "User-Agent", name)` (`Core/HTTP/Client.jet:483`), `sign_extend(bits, 64)` (`Core/Encoding/Binary.jet:175`), `byte_text(bs, 0, start)` (`Core/Encoding/XMLEngine.jet:3573`), `app_page_write(number, self.name, name, bytes)` (`Core/App/App.jet:5385`), `fma(c, x, y)` (`Core/Math/Math.jet:14932`) |
| `Compiler/` (non-runtime, ~200k LOC) | 4 | 4 | 0 (see V2 note) | `x64_decode_hex_parse(bounds[0], text)` (`Compiler/JetBackend/Source/X64/Decoder.jet:1636`) |
| `Compiler/JetRuntime/` | ~142 | ~137 | 5 | `sync_store(value, I64{0})`, `rt_text_store(value, I64{0})`, `host_call(SYS_KILL, number, ...)`, `jet_rt_string_eq(right, rt_static_text(...))` |
| `Jetpack/CLI/` | 0 | 0 | 0 | — |
| `Tests/Guards/` | 0 | 0 | 0 | — |
| **Total (V1)** | **~228** | **~205** | **23** | **~90% false positives** |

*Note:* The V1 sweep above stopped reporting later Compiler hits once `JetBackend` or `Core` hit errors in a dependency; full V2 sweep below measured every Compiler package cleanly.

**Verdict: Attack 1 lands decisively.** Requiring callers to write `set_header(client, "User-Agent", value: name)` or `sign_extend(n: bits, 64)` directly violates P23's promise ("at no cost to correct code").

**Redesign (V2 — directed permutation cycles of length >= 2):**
Build the directed functional graph over the call's eligible parameters where an edge `slot_i -> slot_j` (`slot_i != slot_j`) exists iff:
1. `slot_i` received an unlabeled, non-spread `.Written` argument whose bare variable name (including `^x`, `^^x`, `x.move()`, and `x.copy()`) equals `params[slot_j].label`, and
2. `params[slot_i]` and `params[slot_j]` have the same resolved type (`required_types[i] == required_types[j]`).

Emit **`E0773`** (`Jet.CrossedArgumentNames`) once for every **directed cycle of length >= 2** in this graph:
- A 2-argument swap (`copy(dst, src)` into `fn(src, dst)`, `atan2(x, y)` into `fn(y, x)`, `rect(height, width)` into `fn(width, height)`) forms a 2-cycle and reports **once** across both arguments.
- A k-argument rotation (`rotate(b, c, a)` into `fn(a, b, c)`) forms a k-cycle and reports **once** across the rotated arguments.
- Every one-way coincidence with a literal or compound expression (`set_header(client, "User-Agent", name)`, `sign_extend(bits, 64)`, `byte_text(bs, 0, start)`, `sync_store(value, I64{0})`, `x64_decode_hex_parse(bounds[0], text)`) has out-degree 0 at the literal/expression slot, so it forms **no cycle** and produces **zero false positives**.
- Same-variable duplication (`rect(width, width)`, `span(end, end)`) produces a self-loop on the matching slot (excluded because `other_slot != slot`), so it forms **no cycle**.

**Honest limitation of V2:** A partial swap where one side is transformed inline, such as `copy(dst, src.trim())` into `fn(src, dst)`, does not form a bare-identifier cycle and is not flagged. We accept this boundary deliberately rather than imposing 200+ false positives on idiomatic calls.

### Attack 2: Positional-only parameters (`/`) have no label opt-out

**Attack.** A parameter declared before `/` (`ParamZone.PositionalOnly`) keeps its name private ("so their names stay free to change") and rejects labels with `E0767` (`Jet.LabelOnPositionalParameter`). Flagging a crossed argument on a positional-only parameter would both leak a private parameter name into the public API and suggest a label fix that fails with `E0767`.

**Fix.** Exclude `param.zone == ParamZone.PositionalOnly` and `param.label == ""` (unnamed function-type parameters like `fn(Int, Int)`) from both endpoints of the crossed-name graph.

### Attack 3: Field projections (`area(a.height, b.width)` / `dist(p1.x, p2.x)`)

**Attack.** Extracting the leaf field name from member projections would fire on `area(a.height, b.width)` and `compare(a.x, b.x)` whenever two different structs' fields are compared or combined.

**Fix.** Restrict argument name extraction (`sema_call_crossed_arg_name`) to bare identifiers (`Ident`), explicit `Copy(Ident)`, and zero-argument `.copy()`/`.move()` on a bare `Ident` (along with call-site `^`/`^^` marks, which preserve `arg.expr == Ident`).

### Attack 4: Trailing positional arguments break naive label-only fix hints

**Attack.** When crossed arguments sit in the middle of a call followed by more positional arguments — e.g. `authority_rotate(next, old, running, old_draining, ch)` — writing `authority_rotate(old: next, next: old, running, ...)` fails with `E0768` (`PositionalAfterLabel`).

**Fix.** Detect whether any unlabeled `.Written` argument follows the crossed cycle's last span. When one does, the `Fix:` line explicitly instructs: *"Reorder them to `(...)`, or label them (and any following arguments) explicitly starting with `...`."*

### Attack 5: Auto-fix safety (`no_fix: .Ambiguous`)

**Attack.** Should `E0773` attach a `Safe` `TextEdit`?

**Answer.** No. The compiler cannot distinguish an accidental swap (`copy(dst, src)` meant to be `copy(src, dst)`) from an intentional reversal (`authority_rotate(old: next, next: old, ...)`). Auto-swapping breaks intentional reversals; auto-labeling cements real bugs. Registering `no_fix: .Ambiguous` with clear `fix_next` guidance matches `E0772` (`AmbiguousCall`).

### Attack 6: Error cascading on already-broken calls

**Attack.** If a call already failed argument typing (`E0112`) or name resolution (`E0101`), emitting `E0773` adds noise. And when `E0773` fires on an otherwise well-typed call, returning `None` from `sema_call_typecheck_bound_arguments` would trigger spurious downstream ownership/mutability errors (`E0514`).

**Fix.** Run `sema_call_check_crossed_arguments` only when `complete == true`, push `E0773` as a compilation error (`sema_error`), and still return `Val(SemaCallTypedArguments{...})` so downstream checking completes cleanly without cascades.

## 4. Full-repo empirical sweep under V2 (before fix)

Swept using the live modified `JetSema` across all 6 repo areas:

| Repo area | Files / scope | V2 `E0773` hits | Unique files | Classification |
| --- | --- | ---: | ---: | --- |
| `Core/` | All Core modules & bodies (`jet check Core`) | 21 | 12 | All 21 are intentional literal reversals (`difference(b, a)`, `issubset(b, a)`, `uint_lt(y, x)`, `des_round(y, x)`, `sub_abs(b, a)`, `point_add(q, p)`, `entries_before(right, left)`, `authority_rotate(next, old)`, `binary_compare_coll_seq(r, l)`, `complex_core_ln_abs(b, a)`, `fixed_less_magnitude(b, a)`, `fixed_add(b, a)`, `VdbeData` `b, a` ops) |
| `Compiler/` (non-runtime) | All compiler packages (`JetCLI`, `JetDriver`, `JetSema`, `JetIR`, `JetBackend`, `JetLexer`, `JetParser`, `JetTestKit`, etc.) | 12 | 7 | All 12 are intentional reversals (`ir_map_order(right, left)`, `x64_ucomiss`/`x64_ucomisd(right, left)`, `newline_between(source, end, start)`, `sema_expression_if`/`sema_stmt_check_unit_if_parts` branch flips, `sema_error_conversion_path_exists(edges, to_ty, from_ty)`, `kit_steps_json_equal(right, left)`, `xp_new(..., b, a)`) |
| `Compiler/JetRuntime/` | Full runtime package | 5 | 2 | All 5 are intentional reversals (`rt_x25519_sub`/`add(b, a)`, `rt_ed25519_add(q, p)`, `fiber_heap_before`/`meld(b, a)`) |
| `Jetpack/CLI/` | Full Jetpack CLI + dependencies | 0 | 0 | Clean |
| `Tests/Guards/` | Full Guards package | 0 | 0 | Clean |
| `Tools/Agent/` | 54 standalone tools | 0 | 0 | Clean |
| `Examples/` | 217 files (`Basics`, `Concurrency`, `Collections`, `Crypto`, `Math`, `Memory`) | 0 | 0 | Clean |
| **Total** | **>500,000 LOC** | **38** | **21** | **100% false-positive rate (38/38 hits were intentional, correct reversals clarified by explicit labels; 0 bugs found; ~0.007% hit rate per LOC)** |

## 4b. Prior art and owner ruling

- **Clang-Tidy `bugprone-swapped-arguments`** (LLVM): warns when call-site argument names/expressions are strongly similar to swapped parameter names of compatible types — uses heuristic string similarity and fires as an opt-in lint rather than a language rule.
- **Clang-Tidy `bugprone-easily-swappable-parameters`** (LLVM): flags declarations with adjacent same-typed parameters (like Jet's rejected draft that penalized `pow(x, 2)` and `atan2(y, x)`).
- **Swift argument labels** (SE-0046 / Swift Language Guide): makes labels mandatory by default unless `_` opts out; prevents transposition globally at the cost of verbosity on every call.
- **Kotlin / Python named arguments**: opt-in labels with no language-level crossed-name check; relies on IDE hints or linters.
- **Jet's choice (owner-ratified 2026-11-26):** Require directed permutation cycles of length >= 2 across eligible same-typed parameters (`E0773`, `Jet.CrossedArgumentNames`). The naive one-way rule produced ~205 false positives on idiomatic calls (`set_header(client, "User-Agent", name)`, `sign_extend(bits, 64)`); the owner explicitly ratified narrowing `#7020` to swap/rotation cycles and aligning `Diagnostics.jet` + `Docs/Spec/Reference/syntax-surface.jet` to match.

## 5. Cutover checklist

1. **Diagnostics registry:** Register `E0773` (`Jet.CrossedArgumentNames`) in `Compiler/Prelude/Diagnostics.jet` with text matching the ratified swap-cycle rule, regenerate `Compiler/JetFoundation/Source/Registry/DiagnosticRows.jet` and `Docs/Site/Dist/e/Jet.CrossedArgumentNames/index.html` via `Tools/Agent/DiagnosticRows.jet --write`, and update `Tests/diagnostics_census.tsv` via `Tools/Agent/DiagnosticCensus.jet --write`.
2. **Sema check:** Implement `sema_call_check_crossed_arguments` in `Compiler/JetSema/Source/Sema/Calls/Checking.jet` with zero `.copy()` additions and zero text-keyed lookups.
3. **Full-repo sweep fixes (atomic with cutover):** Sweep `Core/`, `Compiler/`, `Jetpack/`, `Tests/` (including `Tests/Run/`, `Tests/UI/`, `Tests/Conformance/`, `Tests/Cases/`), `Tools/` (including `Tools/Agent/`, `Tools/AgentEval/`, `Tools/Gauntlet/`, `Tools/Perf/`), and `Examples/`, labeling every intentional reversal site so `jet check` passes with 0 `E0773` hits.
4. **Tests, example, and spec:**
   - Unit tests in `Compiler/JetSema/Tests/CrossedArgumentFixtures.jet`
   - UI snapshot pair `Tests/UI/crossed_argument_names.jet` and `Tests/UI/crossed_argument_names.stderr`
   - Golden example `Examples/Basics/CrossedArgumentNames.jet` and `.out` (verified at O0, O1, O2)
   - Spec & reference updates in `Docs/Spec/spec.md` and `Docs/Spec/Reference/syntax-surface.jet`. No new syntax, so no parser, formatter, or editor grammar changes are needed.
