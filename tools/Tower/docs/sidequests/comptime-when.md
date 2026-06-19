# Compile-time `if` (Odin-style `when`)

**Status:** Draft plan — needs owner review (2026-06-19)
**Card:** c24

## Problem & why it matters

Odin's `when` is a compile-time conditional: the branch is selected during
compilation, the unselected branch is never type-checked against the target,
and nothing about it survives into the binary. The card asks for that — a
construct for **conditional compilation** and **static dispatch** keyed off a
compile-time-known value (a target flag, a build profile, a comptime constant,
a numeric platform width).

Today Jet has no such thing. Jet has:

- runtime `if` (S68/D-IF1) — both branches are checked, the chosen one runs at
  runtime, both are in the binary;
- `comptime x = f();` bindings (S26/S57) — pure value-level evaluation, **no
  blocks, no branches, no annotations** (v1 scope explicitly excludes them).

So a user who wants "compile only this block when building for freestanding"
or "pick this implementation on a 64-bit target" has no spelling. They fall
back to runtime `if` and pay for dead branches that may not even *compile* on
the off-target (e.g. a 64-bit-only intrinsic referenced on a 32-bit build).

**Naming trap (must flag):** the card title says "`when`". `when` is **retired
in Jet** — D-IF1 (2026-06-18) folded multi-arm dispatch into `if` and `when`
now raises `E_KEYWORD_RETIRED` pointing at `if`. We cannot reuse `when` as the
keyword. The plan below proposes **`comptime if`** as the canonical spelling so
it reads as "the compile-time form of the one branching keyword" and reuses an
already-ratified word.

**Scope trap (must flag):** S26 fixed two laws this feature must not break.
(1) "comptime never creates, parameterizes, or selects a **type**, and never
affects **dispatch**" — so `comptime if` may select *code/values*, but the
arms must not change the program's type-level shape (no comptime trait
selection, no comptime generic instantiation). (2) S57 set v1 comptime scope to
"bindings only — no comptime blocks." `comptime if` is a **block-introducing
control form**, which is exactly the thing S57 deferred. This feature is
therefore an explicit **extension of ratified comptime scope** and needs owner
sign-off before any code (I8).

## Prior art (terse)

- **Odin `when cond { … } else when … { … } else { … }`** — compile-time `if`.
  Condition must be a compile-time constant; the unselected branch is parsed
  but **not semantically checked**; used for OS/arch/config branching.
- **Zig `comptime` + `if` on `comptime`-known values** — the same effect falls
  out of Zig's general comptime; the dead branch is still elided. Jet refuses
  full Zig comptime (S26 "Rejected forever") so we want the *narrow* Odin form,
  not Zig's general mechanism.
- **Rust `#[cfg(...)]`** — attribute-driven conditional compilation. Powerful
  but it is a second, attribute-shaped mini-language; against Jet's
  one-mechanical-path priority and against the readability goal.
- **C `#if/#ifdef`** — preprocessor text substitution; the footgun baseline we
  are explicitly better than.

## Proposed design (worked Jet example)

`comptime if <comptime-bool> { … } else if <comptime-bool> { … } else { … }`.
The condition must evaluate under the existing comptime interpreter (S26
Layer 1: pure, deterministic, no IO/FFI). The **selected** arm lowers normally;
the **unselected** arm is parsed and name-resolved but **not type-checked
against its surroundings and not lowered** — that is the whole point (it may
reference a target-only intrinsic that does not exist off-target).

```jet
// A compile-time build flag, known at comptime (S26/S57 binding).
comptime wide = target.pointer_bits == 64;

fn checksum(buf: List<Int>) -> Int {
    comptime if wide {
        // Only compiled on 64-bit targets. A 64-bit-only intrinsic here
        // never reaches codegen on a 32-bit build, so it can't fail to link.
        fold_u64(buf)
    } else {
        fold_u32(buf)
    }
}
```

Statement position, for conditional *items* / blocks:

```jet
comptime if profile == "freestanding" {
    use core.mem;                 // only pulled in for the freestanding build
    mem.set_allocator(arena);
}
```

How it differs from runtime `if`:

| | runtime `if` (S68) | `comptime if` (proposed) |
|---|---|---|
| condition | any `Bool` | must be comptime-evaluable |
| both arms checked? | yes | **no** — only the selected arm |
| both arms in binary? | yes | **no** — unselected arm is dropped |
| expression form | yes (D-SG2) | yes, selected arm's value |
| can it pick a type / affect dispatch? | n/a | **no** (S26 law) |

Interaction with `comptime` bindings: the condition is exactly a comptime
expression, so `comptime` constants, `embed_file`, and pure `fn` calls all
compose. A `panic` reached while evaluating the *condition* is a compile error
(S26 comptime-panic feature). A `panic` lexically inside the **unselected** arm
is **not** evaluated (the arm is never run, never lowered).

## Implementation sketch — pipeline touchpoints

1. **`Source/Syntax.rs` (I7).** No new keyword — reuse `comptime` (S57) + `if`
   (S68). Record the *combination* `comptime if` under the S26/S57 block with
   the new decision ID (D-WHEN1). Add `E_KEYWORD_RETIRED` cross-ref note that
   `when` still points at `if`, unchanged.
2. **Parser.** Add a `comptime`-prefixed `if` production: when the parser sees
   `comptime` followed by `if`, build a `CompTimeIf` node (arms = parsed-but-
   unchecked blocks). Reuse the existing `if`/`else if`/`else` chain parser; the
   only delta is the leading `comptime` and a node-kind flag. Braceless arm
   bodies (D-IF2) should *not* apply here — require `{ }` arms for clarity
   (decide: D-WHEN1 Open).
3. **Sema.** New pass step: (a) type-check + comptime-evaluate every condition
   in source order using the existing tree-walking interpreter
   (`Source/Sema.rs` comptime path); (b) select the first true arm (or `else`);
   (c) type-check **only** the selected arm against its context; (d) for
   unselected arms, run name-resolution-only (so typos still teach) but skip
   type-checking and skip lowering. Enforce the S26 dispatch law: reject any
   arm that would introduce a trait impl, derive, or generic instantiation
   visible outside the arm (E-band below).
4. **Codegen (dumb, I3).** Receives only the selected arm — emits it as an
   ordinary block/expression. The unselected arm never reaches codegen, so no
   `#[cfg]` is emitted; conditional compilation is resolved entirely in the
   front end (I2/I3 preserved — rustc never sees a dead off-target branch).
5. **`jet fmt`.** Format `comptime if` like `if` with the `comptime` lead token;
   one sibling arm in `Source/Formatter.rs`.

## Test plan — ui snapshots + example

- **Example (I5):** `examples/features/NN_comptime_if.jet` — the `checksum`
  example above, with a fixed comptime flag so output is deterministic; `.expected`
  golden output.
- **ui snapshot — non-comptime condition:** a `comptime if` whose condition
  reads a runtime `var` → new diagnostic E (band E07xx, the comptime family) —
  "this `comptime if` condition must be known at compile time". Snapshot pinned
  (I4).
- **ui snapshot — unselected arm not checked:** an example where the unselected
  arm calls an undefined-on-this-target name and **still compiles** (proves the
  arm is dropped); plus a sibling proving a **typo in the unselected arm** is
  still caught by name-resolution (the teaching guarantee).
- **ui snapshot — dispatch law:** an arm that tries to add a trait impl /
  derive → rejected with the S26-law diagnostic (band E07xx). Snapshot pinned.
- **Differential CI (S26):** the comptime interpreter's branch selection must
  match a runtime-`if` oracle on every evaluable condition.

## Risks & invariant check

- **I1** safe + expert: no new unsafety; pure front-end mechanism. OK.
- **I2** rustc silent: the off-target arm never reaches rustc → no rustc error
  surface. **Strengthens** I2 vs. runtime `if`. OK.
- **I3** codegen dumb: selection happens in sema; codegen sees one arm. OK.
- **I4** every new diagnostic gets a code + snapshot (listed above). OK.
- **I6** zero crates in Source/: pure std Rust. OK.
- **I7** keyword/sigil with ID: no new token; `comptime if` combination logged
  under D-WHEN1. OK.
- **I8** simplicity ratchet — **this is the live risk.** S57 deferred comptime
  blocks deliberately. Adding `comptime if` reopens that. Justification: it is
  the *minimum* form that delivers conditional compilation (a real
  freestanding/embedded need per philosophy) without opening full Zig comptime
  (still rejected). **Needs owner sign-off** before code — flagged, not assumed.
- **S26 dispatch law:** the single biggest correctness risk is an arm
  smuggling type/dispatch selection. Sema must reject it; the law is what keeps
  this from becoming Zig comptime by the back door.

## Open decisions

1. Keyword spelling — confirm `comptime if` vs. a dedicated word (the card's
   `when` is unavailable; see D-WHEN1).
2. Does the unselected arm get **name-resolution-only** checking (catches typos,
   the recommended teaching stance) or **zero** checking (pure Odin, but a typo
   in a dead arm survives until that arm is selected)?
3. Braceless arm bodies (D-IF2) — allow for `comptime if` or require `{ }`?
4. Statement-position conditional `use` — in scope for v1, or values/exprs only?

## Proposed decision card(s)

### D-WHEN1 — Compile-time conditional spelling (rec A)

Jet has no compile-time `if`. The card asks for Odin's `when`, but `when` is
retired (D-IF1). Below: how the user spells "compile this branch only".

- **Option A — `comptime if` (recommended).** Reuses two ratified words; reads
  as "the compile-time form of `if`." Condition is a comptime expression; only
  the selected arm is checked and lowered.

    ```jet
    comptime if target.pointer_bits == 64 {
        fold_u64(buf)        // only this arm compiles on a 64-bit build
    } else {
        fold_u32(buf)
    }
    ```

- **Option B — bare `comptime { }` block + ordinary `if` inside.** Smaller
  grammar delta, but it conflates "run at comptime" with "select at comptime"
  and reopens the general comptime-block can S57 closed.

    ```jet
    comptime {
        if target.pointer_bits == 64 { fold_u64(buf) } else { fold_u32(buf) }
    }
    ```

- **Option C — `static if` (D / C++ spelling).** Familiar to D users, but
  `static` is an unspent word that would mean *only* this; adds vocabulary for
  one feature against I8, and "static" is jargon (diagnostics voice bans it).

    ```jet
    static if target.pointer_bits == 64 { fold_u64(buf) } else { fold_u32(buf) }
    ```

- **Option D — reject; tell users to use runtime `if`.** Simplest (I8 default
  answer is no). Cost: no conditional compilation → off-target intrinsics can't
  be guarded → forecloses the freestanding/embedded story philosophy commits to.

    ```jet
    if target.pointer_bits == 64 { fold_u64(buf) } else { fold_u32(buf) }
    // both arms compiled; fold_u64 must link on every target — the blocker.
    ```

**Recommendation: A.** It reuses ratified words, reads plainly, and is the
narrow Odin form (not Zig's general comptime, still rejected by S26). Gated on
the owner accepting an extension of S57's "bindings-only" comptime scope.

### D-WHEN2 — Checking of the unselected arm (rec A)

- **Option A — name-resolution only (recommended).** The dropped arm is scanned
  for unknown names so typos still teach, but it is not type-checked against its
  surroundings (an off-target intrinsic is allowed). Keeps Jet's teaching
  guarantee without forcing the dead arm to type-check.

    ```jet
    comptime if false {
        wobble(x)        // E: nothing named `wobble` exists  (still caught)
    } else {
        ok(x)
    }
    ```

- **Option B — zero checking (pure Odin).** The dead arm is parsed and ignored.
  A typo survives silently until that arm is selected on some other build.

    ```jet
    comptime if false {
        wbidth_64()      // typo passes today, breaks the 64-bit build later
    } else { ok(x) }
    ```

**Recommendation: A** — matches Jet's "diagnostics are the product" priority #2.
