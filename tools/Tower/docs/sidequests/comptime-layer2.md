# comptime Layer 2 — compile-time pure evaluation + data embedding

**Status:** Draft plan — needs owner review (2026-06-19)
**Card:** c61

## What's ratified / the goal

S60 was re-ratified **to pursue** on 2026-06-19 (decisions log line 2041):
"compile-time pure evaluation + data embedding — `comptime` Layer 2 promoted
from post-1.0." The goal is to make Jet's compile-time evaluation a
first-class, mainstream capability: run pure Jet at build time, **embed the
evaluated result into the binary as constant data**, and give a clean,
single impurity story when a comptime computation reaches the outside world.

### Naming reconciliation (must flag — two definitions of "Layer 2")

The phrase "Layer 2" is overloaded and the plan must not silently pick one:

- **S26 layering (ratified, line 617–631):** Layer 1 = `comptime` bindings
  (M9.5, shipped); **Layer 2 = built-in derives (M9)**; Layer 3 =
  reflection / user derives (Epoch 3).
- **The S60 card / this card:** "comptime **Layer 2** = compile-time pure
  evaluation + data embedding."

These are different axes. To avoid contradicting the spec the owner reads
this against, this plan treats the card's "Layer 2" as **the pure-evaluation
+ data-embedding capability tier**, not the S26 derive layer. Recommendation:
when this lands, log it as an extension of **S60** and cross-reference S26 so
the two "Layer 2"s don't collide in future docs. (This is the single most
likely thing the owner will want to settle first.)

## Current state to build on (cite files — much already exists)

The framing "build on existing comptime work, extend not greenfield" is exact:
the engine, the impurity check, and even a pure-program entry point already
ship. What's missing is *promotion* (richer embeddable data, mainstream build
path, one impurity story), not a new interpreter.

- **The evaluator** — `Source/Comptime/` (`Interpreter.rs`, `Builtins.rs`,
  `Methods.rs`, `Value.rs`, `Purity.rs`, `Diagnostics.rs`, `mod.rs`). A
  tree-walking interpreter over the typed AST. Entry points already exist:
  `evaluate` / `evaluate_owned` (comptime bindings), `run_main*` (`jet dev`),
  `run_repl_step` (REPL). One interpreter, reused everywhere (I2 differential
  guarantee).
- **`comptime` bindings (Layer 1)** — `Source/Sema/CheckerCore.rs` ~1501
  (`b.is_comptime` → `Comptime::evaluate_owned`), `Source/Sema/Bundle.rs`
  ~431 (`eval_comptime_items`). Result is stored on the binding and lowered
  by `Source/Codegen/Statement.rs` ~129 via `CtValue::serialize()` → plain
  Rust constant data. Codegen is dumb (I3).
- **Embeddable value shapes — already broad.** `CtValue`
  (`Source/Comptime/Value.rs` 11) already has `Struct` and `Enum` variants,
  and `serialize()` (173–202) already emits Rust struct/enum literals. So a
  comptime-computed struct/enum *already* bakes into the binary today — this
  is **not** a Layer-2 gap (contrary to a first read of the card).
- **Comptime method vocabulary — the actual gap.**
  `Source/Comptime/Builtins.rs` `apply_method` (140) supports `String.split`,
  `len`, `trim`, `replace`, `contains`, `starts_with`, `chars`, list/map
  methods, and numeric conversions — but **no `String.lines()` and no
  `String.to_int()`** (only `Float.to_int`). Turning embedded text into typed
  data needs these added.
- **`embed_file("path")`** — the one sanctioned impure-looking builtin;
  bakes file bytes/text at compile time (`examples/features/29_embed.jet`).
  E0955 covers missing/unreadable/non-UTF-8.
- **`pure fn` (S60, implemented E2-M16)** — `Source/Syntax.rs` `KW_PURE`
  (381), `f.is_pure` on the AST. A checked modifier: a `pure fn` may only
  call other `pure fn`s and pure builtins.
- **`jet eval --pure`** — `Source/CmdDevTools.rs` `run_eval` (305) +
  `jet::eval_pure_program` (`Source/lib.rs` 309): requires every top-level fn
  `pure`, runs `main`
  through the comptime interpreter, prints stable JSON. This is the Layer-3
  ("replace Nix") path, already wired at a basic level.
- **Impurity diagnostics — TWO families already exist:**
  - **E0951** (`Source/Comptime/Diagnostics.rs` `impurity_diag`) — comptime
    code reaches an impure op, with the call path. Plus E0952 (fuel), E0953
    (comptime panic = user compile error), E0955 (embed errors), E0956
    (construct unsupported at comptime).
  - **E3401/E3402/E3403** (`Source/lib.rs` ~333, `Source/CmdDevTools.rs`
    ~346, `Jetpack/ModuleEval`) — pure-eval / `pure fn` / sandboxed-build
    impurity. E3401 *is* emitted today (the `jet eval --pure` gate).
  Layer 2 must pick which family its impurity case uses and **not mint a
  third** (see Open decisions).
- **Differential battery** — `tests/comptime_diff.rs`: the comptime
  interpreter and the compiled runtime must agree bit-for-bit on every
  evaluable expression. Any new evaluable construct or embeddable data shape
  extends this file (I3 enforcement).

## What Layer 2 adds over today

Today's comptime is real but narrow: `comptime X = expr` binds a single
constant; `embed_file` brings in a file's text. Layer 2 broadens this to a
**general compile-time data-embedding tier**:

1. **Richer comptime parse/text builtins — the real engine gap.** Struct and
   enum *serialization* already exists (`CtValue::Struct`/`Enum` and their
   `serialize()` arms, `Source/Comptime/Value.rs` 19–23, 173–202), so a
   comptime-computed struct *already* bakes into the binary today. What's
   missing is the **vocabulary to turn embedded text into typed data**: the
   comptime method table (`Source/Comptime/Builtins.rs` `apply_method`) has
   `String.split`, `len`, `trim`, `replace`, `contains`, `chars`, but **no
   `String.lines()` and no `String.to_int()` / `parse`** (only `Float.to_int`
   exists). Adding those pure builtins is what lets
   `comptime ROWS = parse_rows(embed_file("app.csv"))` produce a typed
   `[Row]` rather than dead-ending at E0956.
2. **Structured-file embeds.** With the parse builtins above, embedded data
   arrives typed, not as a `String` the program re-parses at startup. (The
   struct it lands in already serializes — see point 1.)
3. **One impurity story.** Unify E0951 (comptime-binding) and E3401
   (pure-eval) presentation so a user sees the same teaching whether the
   impurity is hit in a `comptime` binding or a `pure fn` (see Open
   decisions — recommend keep both codes, share the message builder).
4. **Promotion to the mainstream build path.** Make the embedding tier a
   documented, exampled feature beginners can reach (a `comptime` table, an
   embedded asset), not a post-1.0 footnote.

**Scope trap (must flag):** S57 fixed v1 comptime to "**bindings only — no
comptime blocks, parameters, or function annotations**." Layer 2 as scoped
here stays inside that: it works through `comptime X = …` bindings and
`pure fn`, both already-ratified syntax. It does **not** introduce
`comptime { … }` blocks. If a future iteration wants blocks, that is a
separate syntax decision (see the `comptime-when.md` sidequest), not part of
this card. No new user-facing syntax here.

## Proposed implementation (worked Jet example)

```jet
// app.csv embedded and parsed at COMPILE time; the binary ships the table
// as constant data — no file open, no parse at runtime.
struct Row {
    name: String
    weight: Int
}

pure fn parse_rows(text: String) -> [Row] {
    rows: [Row] := []
    loop line in text.lines() {          // NEW comptime builtin (see below)
        parts :: line.split(",")         // already supported at comptime
        rows.push(Row{name: parts[0], weight: parts[1].to_int()})  // NEW: String.to_int
    }
    return rows
}

comptime ROWS = parse_rows(embed_file("app.csv"))   // embed_file: supported today

fn main() {
    print("baked {ROWS.len()} rows at compile time")
    print("{ROWS[0].name}: {ROWS[0].weight}")
}
```

`app.csv`:

```
oxygen,16
helium,4
```

Expected output:

```
baked 2 rows at compile time
oxygen: 16
```

**What is actually new here.** `ROWS` is a `[Row]` — a list of struct values
— and that **already serializes** into constant Rust data today
(`CtValue::Struct` + `serialize()`). The two methods marked NEW are the
genuine engine work: `String.lines()` and `String.to_int()` do not exist in
the comptime method table yet, so without them this `pure fn` dead-ends at
**E0956** ("can't run at compile time yet"). Layer 2 = adding the pure parse
vocabulary that lets embedded text become typed data, then promoting the
whole flow to a documented, exampled, mainstream feature. If a `pure fn` in
the chain reaches an impure call, the build stops with the impurity
diagnostic (E0951), call path included.

## Implementation sketch — file-level pipeline touchpoints

**parser** — no change. `comptime`, `pure fn`, and struct/enum literals all
parse today.

**sema** —
- `Source/Comptime/Builtins.rs` `apply_method` (140): add the missing pure
  String builtins — `lines` (→ `List<Str>`), `to_int` on `Str` (→ `Int`,
  with a comptime parse-error path), and any others the flagship examples
  need. Mirror the *runtime* String methods exactly so the differential
  battery stays green. `CtValue::Struct`/`Enum` and their `serialize()` arms
  (`Value.rs` 19–23, 173–202) already exist — **no new CtValue variants
  needed**.
- `Source/Comptime/Purity.rs`: unchanged in shape; it already rejects the
  first impure call with the path. The new builtins are pure, so they must
  *not* be added to `impure_builtin`.
- `Source/Sema/Bundle.rs` / `CheckerCore.rs`: the binding-eval path already
  calls the interpreter and stores the result; no structural change. The
  promotion work here is documentation + examples + the `jet eval` surface,
  not new plumbing.

**codegen** — none. `Source/Codegen/Statement.rs` ~129 already lowers `b.ct`
via `CtValue::serialize()`, and that already emits Rust struct/enum literals
(`user_Row { user_name: "oxygen".to_string(), user_weight: 16i64 }`). Codegen
stays dumb (I3/R1).

**diagnostics** — no *new* code if we reuse E0951 for comptime-binding
impurity and E3401 for `pure fn`/`jet eval` impurity (both exist). Layer 2's
job is to share the **message builder** so both read identically. E0955
(embed) and E0956 (unsupported-at-comptime) already cover the embed and
not-yet-supported cases; E0956 fires naturally if a construct in a comptime
chain isn't evaluable yet.

## Test plan — ui snapshots + example(s)

- **Example (I5):** new `examples/features/30_comptime_embed_struct.jet` (the
  CSV→`[Row]` case above) + `app.csv` + `expected/30_*.out`. Golden test:
  front-end-passes, no `unsafe`, prints the baked rows.
- **Differential (the load-bearing test):** add cases to
  `tests/comptime_diff.rs` asserting the interpreter and the compiled run
  produce the **same** struct/enum/list-of-struct values bit-for-bit. This is
  the I3 guard for every new serialize path.
- **ui snapshot — impurity in a comptime chain:** `tests/ui/comptime_impure.jet`
  (a `pure fn` chain that reaches `print`/an extern) → E0951 with the call
  path. If a separate `pure fn` / `jet eval --pure` snapshot is wanted, pin
  E3401 too.
- **ui snapshot — unsupported construct:** a comptime chain touching a
  not-yet-evaluable construct → E0956.
- **Codegen golden:** confirm the generated Rust for the struct embed is a
  plain literal (no runtime parse, no `unsafe`).

## Risks & invariant check

- **I2 (rustc never speaks to users) / differential drift:** the single
  biggest risk is the new parse builtins (`String.lines`, `String.to_int`)
  computing a *different* result at comptime than the compiled runtime does
  — e.g. line-splitting on `\r\n` vs `\n`, or `to_int` accepting whitespace
  the runtime rejects. The fix is to back each comptime builtin with the
  exact same logic the runtime uses. `tests/comptime_diff.rs` must gain a
  case for *every* new builtin — non-negotiable. (Struct/enum serialize is
  already covered and unchanged.)
- **I3 (dumb codegen):** all evaluation in the interpreter; codegen only
  prints `serialize()` output. Honored.
- **I4:** no new diagnostic invented if we reuse E0951/E3401; if the owner
  prefers a dedicated Layer-2 code, it ships with a diagnostics.md row +
  snapshot.
- **I6 (zero crates in `Source/`):** the comptime parse builtins must be
  native Jet/Rust (std-only) — do **not** pull a CSV/TOML crate into the
  compiler. Stdlib ring-library parsers (E2-M9) may back the *runtime* side,
  but the comptime path must use compiler-internal pure code.
- **S26 one law:** comptime computes **values only** — no type creation,
  parameterization, or dispatch. The struct/enum values here are data; the
  *types* `Row`/`Config` are declared in ordinary source. Honored.
- **S57 scope:** no `comptime { }` blocks, no comptime params/annotations.
  Layer 2 works through bindings + `pure fn` only. Honored.

## Open decisions — genuine engineering forks only

1. **Impurity-code unification.** Three options:
   - keep **E0951** (comptime bindings) and **E3401** (`pure fn` / eval) as
     two codes but share one message builder (recommended — both already
     exist and have distinct `jet explain` entries);
   - collapse to one code (loses the binding-vs-pure-fn context);
   - mint a third Layer-2 code (rejected — redundant, violates the
     "registered + honest" economy).
   **Recommendation: keep both, share the builder.** No new code, consistent
   teaching.

2. **Which pure parse builtins ship in the comptime method table first.**
   Engineering scoping, not syntax: start with the minimum the flagship
   example needs (`.lines()`, `.split()`, `.to_int()`), add more as examples
   demand (I8 ratchet). No owner syntax decision required.

3. **"Layer 2" label reconciliation** (see top section) — the one item that
   benefits from an owner word: log this as an **S60 extension**
   cross-referencing S26, so the two Layer-2 meanings don't collide.
   **Recommendation: S60-extension framing**; no code until the owner
   confirms the label.

No user-facing syntax is unresolved (S60 / S57 / S26 all ratified; this plan
introduces no new spelling).

## Proposed decision card(s)

**No syntax decision is needed** — S60 (the capability), S57 (bindings-only
scope), and S26 (comptime computes values only) are all ratified, and this plan
adds no new spelling. The only owner-facing call is a *labelling* one: the word
"Layer 2" already means two different things in the spec, and the implementation
should not silently pick one. That single reconciliation is drafted below. The
two engineering forks (impurity-code unification; which parse builtins ship
first) are scoping calls the implementer can make under the I8 ratchet — they do
not need an owner ballot.

### D-CT-L2NAME — Reconcile the two "Layer 2" labels (rec A)

"Layer 2" is overloaded across ratified spec text and the implementer needs to
know which doc the new work attaches to, so future readers aren't told two
contradictory things:

- **S26 layering** (`docs/spec/syntax-decisions.md:629-630`): Layer 1 =
  `comptime` bindings; **Layer 2 = built-in derives**; Layer 3 = reflection /
  user derives.
- **S60 card / card c61** (`docs/spec/syntax-decisions.md:2041`): "comptime
  **Layer 2** = compile-time pure evaluation + data embedding."

Both are ratified phrasings on *different axes* (S26 = the derive-machinery
ladder; S60 = the pure-eval capability tier). This card only decides how to label
the work when it lands — no behaviour changes either way.

- **Option A — log it as an S60 extension; cross-reference S26 (recommended).**
  The pure-eval + embedding work is filed under **S60**, and the spec entry adds
  a one-line note that "Layer 2" here is the S60 *capability tier*, distinct from
  the S26 *derive layer*. No renaming of existing ratified text.

    ```jet
    // Doc/changelog framing only — code is identical under any option:
    // S60 ext: comptime pure-eval + data embedding ("capability Layer 2",
    //          not the S26 derive Layer 2). String.lines()/to_int() added;
    //          CtValue::Struct/Enum already serialize.
    comptime ROWS = parse_rows(embed_file("app.csv"))
    ```

- **Option B — rename one axis to drop the collision.** Re-label the S60 tier
  (e.g. "comptime *embedding tier*") or the S26 layer so the word "Layer 2" is
  used by exactly one of them. Cleanest end state, but it edits ratified spec
  prose and breaks existing references to "Layer 2" in the S26 sense.

    ```jet
    // Same code; the only change is the spec calls this the
    // "comptime embedding tier", reserving "Layer N" for the S26 derive ladder.
    comptime ROWS = parse_rows(embed_file("app.csv"))
    ```

**Recommendation:** **A** — attach to S60 with an explicit cross-reference to
S26. It settles the ambiguity for future readers without re-opening or rewording
any ratified decision, and keeps the derive-layer numbering (S26) intact.
