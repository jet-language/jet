# A `:=` binding that never changes

Date: 2026-10-30. Card #6324. Probes live in
`/mnt/jetscratch/lanes/SurfaceDesign/probes/` (`m6324`, `m6324b`, `m6324x`).
Every probe printed the same output on o0, o1 and o2 with the base compiler at
bd83fe6361.

## What Jet does today

`::` binds a value that never changes and `:=` binds one that can
(`Docs/spec/syntax-decisions.md:21-22`). `name = expr` reassigns (lines 237-241).
Typed bindings are retired; a type rides the value (lines 244-248).

A `:=` that is never written compiles silently today. `x := 1; print(x)` prints
1 with no diagnostic under `jet run`, `jet run --verbose` and `jet check`
(`m6324/a01`). An unused `x := 1` is also silent (`a25`).

The lint for this case is registered but has no emitter:

- L0528 `mutable_never_reassigned` is a warning (severity 2) with a
  formatting-safe source edit, "Declare it with `::` instead of `:=`"
  (`Compiler/JetFoundation/Source/Registry/DiagnosticRows.jet:4690-4705`,
  `Compiler/Prelude/Diagnostics.jet:391`).
- A text search of `Compiler/` finds the code only in the registry rows and the
  syntax dictionary (`Compiler/JetCli/Source/Cli/SyntaxDictionaryData.jet:300`).
  The dictionary still promises "a hint to use `::`".
- The Rust checker emitted it until c9129d174d (2026-10-23). Its last rule is at
  618f5fa974, `crates/jet-sema/src/Sema/CheckerInfer/stdlib_lints/loop_lints.rs:166-212`.
- The parity triage lists it as missing
  (`Docs/audits/rust-exit-parity/triage.tsv:2579`).
- Two UI snapshots still expect it: `tests/ui_lint/arrow_loop_value_drop.warn:1-8`
  and `tests/ui_lint/busy_spin.warn:10-26`.

The neighbouring lints L0529 (state loop never changes its header) and L0302
(same-enum guard table) are also rows with no emitter found by text search.
L0153 (unreachable typestate) does fire under `--verbose`.

## The old rule

The Rust rule was syntactic and looked at the rest of the enclosing block
(`loop_lints.rs:396-538`). A later statement counted as a write when it held:

- an assignment to the name, or to a field or index rooted at it;
- a `&name` argument to a call;
- any method call on the name, whether or not the method changes it;
- a write inside a lambda body, an `if` branch, a `??` fallback, a loop body, a
  `switch` arm or a compile-time block.

It skipped compiler-generated bodies and bindings with no written sigil
(lines 170-196). It did not check whether a branch could run.

## What counts as a write, probed

| Case | Probe | Result today |
| --- | --- | --- |
| `x = 2`, `x += 1` | a02, a03 | runs |
| `&xs.push(2)` | a04 | runs; without `&` it is E0224 (a05) |
| Field write `p.n = 2`, nested `a.b.n = 9` | a07, a32 | runs |
| Index write `xs[0] = 5`, map insert `m["a"] = 1` | a09, a30 | runs |
| `bump(&x)` into a `&Int` parameter | a11 | runs |
| The same writes on `::` | a06, a08, a10, a12 | E0202 or E0205 |
| Lambda that writes, called / never called | a13, a14 | runs; prints 2 / 0 |
| Write in a branch, or in `if false { … }` | a15, a16 | runs |
| Write inside a `??` fallback block | m6324b/x13 | runs; prints 9 |
| State-loop header written / not written | x04, x05 | both run |
| State-loop header with `::` | x03 | E0372 and E0003 |
| `{ sender: tx, receiver: rx } := channel<Int>()`, never written | x14 | runs |
| `flag :: shared Flag{…}` then a field write | x01, x02 | runs |
| `#Persist counter := 3`, written / never written | x07, x06 | both run |
| Module `counter := 3` written from a function | a23 | prints 4 |
| `comptime { … }` as a value | a20, a21 | E0391, not supported yet |
| Parameter reassigned | a35 | E0111 |

A write through a `Shared` value does not need `:=` (x01, x02). The busy-spin
snapshot already expects L0528 on `flag := shared …` and `changed :=
Condition.new()`.

Probe a23 contradicts the spec. A plain module `:=` written from a function
should be E0111 unless it is `#Persist` (`syntax-decisions.md:8038-8045`).
It runs and prints 4.

D-RECEIVER-MARK1=A (ratified 2026-10-30) drops `&` before the dot. It is not
built: `c.bump()` without `&` is still E0224 (a28). Under that ruling a call
to a changing method is a write even with no mark at the call.

## Sites that would change

A heuristic census script (`b2/census6324.mjs` in the lane directory) finds
local `name := value` lines with no later write in the same indented block. A
strict write is an assignment, a compound assignment, a field or index
assignment, or `&name`. A loose write adds any method call on the name, as the
old rule did. It does not see two bindings on one line, so it undercounts.

| Tree | Files | Local `:=` | Never written (strict) | Never written (loose) |
| --- | --- | --- | --- | --- |
| Core | 170 | 4,978 | 468 | 414 |
| Examples | 1,508 | 1,300 | 273 | 186 |
| Compiler | 1,470 | 22,834 | 4,514 | 3,436 |
| Jetpack | 767 | 7,388 | 90 | 52 |
| Tools | 733 | 1,636 | 100 | 66 |
| tests | 7,857 | 2,464 | 927 | 580 |
| Total | 12,505 | 40,600 | 6,372 | 4,734 |

Spot checks matched the census. `Examples/features/web/ui_showcase.jet:197`
(`pct := …`) and `tests/run/ownership/enums_and_mutable_aggregates_carry_view_provenance.jet:34`
(`right := [9, 10]`) are never written. Twelve state-loop headers and 99 module
`:=` bindings were seen; nine module bindings are never written.

The rewrite is the old fix: replace the `:=` sigil with `::`. It changes no
behaviour. A state-loop header cannot take `::` (x03), so its fix is a hoisted
`name :: value` before a `loop` with the test in the header.

## Other languages

| Language | Never-changed mutable local | Level |
| --- | --- | --- |
| Zig 0.12+ | "local variable is never mutated", note "consider using 'const'" | compile error |
| Rust | `unused_mut`, "variable does not need to be mutable", help "remove this `mut`" | warning, on by default |
| Swift | "variable 'x' was never mutated; consider changing to 'let' constant", with a fix-it | warning |
| Kotlin | IDE inspection "can be declared as 'val'" | editor hint |
| Go | no immutable local exists; `const` is compile-time only | none |

Zig made it an error in 0.12.0 and kept it; the release notes call the fix
"simple: use const". Zig and Go also make an unused local an error. Rust and
Swift kept a warning with a machine fix. Kotlin leaves it to the editor.

Taking an address counts as a change in Zig. A `var` passed by `&x` is
mutable even when the callee only reads it [INFERENCE from the Zig rule text].
Jet has the same choice: `&x` asks for write access, so it counts as a write.

## Ratified versus built

| Item | Ratified | Built |
| --- | --- | --- |
| `::` / `:=` meaning (D-BIND1) | yes | yes |
| L0528 warning | registry row | no emitter since 2026-10-23 |
| L0529 state-loop warning (D-LOOPREAD1=A) | yes | no emitter found |
| Module `:=` write from a function is E0111 | spec text | runs (a23) |
| No `&` before the dot (D-RECEIVER-MARK1=A) | yes | no (a28) |
| `comptime { … }` as a value | spec | E0391 |
