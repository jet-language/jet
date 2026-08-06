# Failure: one report, three routes

Status: proposal for owner decision. Ballots D-FAIL-* on card #1507.
Date: 2026-08-06.

## Executive summary

The owner asked: what is a failure — a recoverable value, a contract breach,
or a fault — and what does each owe the caller?

The research swept the spec, sema, codegen, Prelude, examples, tests, the
decision record, prior audits, and the four sibling rethinks. The finding is
sharp. Jet has one good rail (`T ? E` with `?` and `??`) standing on a hollow
floor. The default `Error` type is a plain string at codegen. The ratified
structured form (message, code, source) was never built. The `Fallible` trait
cannot be implemented by any program; the diagnostic that teaches it (E2402)
gives a fix that is itself a hard error. Contract markers `#Pre`/`#Post` run
only in AOT builds; `jet run` skips them silently, against their own ratified
text. Runtime stops have four renderers with different wording per tier, and
the JIT loses the source location. `#Todo` and every raw panic in the Prelude
exit with code 101 — the code reserved for compiler bugs. The web tier throws
bare JavaScript errors with none of the report. Compile-time failures are
products (I4): 1064 registered codes with what/why/fix. Runtime failures are
four codes and a pile of `eprintln!`.

The one idea: **every failure is one product — a report — delivered on one of
three routes. Attribution picks the route.** If the world broke the promise
(a missing file, bad input), the failure is a value in the signature: the
caller gets a report it can hold, match, and convert. If code broke a promise
(an overflow, a failed contract, an index out of range), the failure is an
attributed stop: the report names the promise, the party, and the site, and
no code catches it. If the substrate broke (a compiler bug, a foreign crash),
the failure is a contained stop: the nearest boundary limits the blast and
keeps the report. Routes change only at spelled boundaries: a task join turns
a stop into a value; the process edge turns a value into an exit code.

This is not a new mechanism. It is the shape Jet's ratified law already has.
The must-use errors (E0401/E0402), the `.drop("reason")` rule, the exit-code
table, the I2 wall, the D-VALIDATE1 three-layer ruling, and the D-INTBIG1
overflow verdict all fall out as theorems of one law: **no failure is lost,
reworded, or rerouted silently — one report, delivered once, on the route its
attribution picks.**

What the owner gets on the page: a real `Error` you can build, read, match,
and chain; `.context` on every fallible value, not just one type; one
conversion rail instead of two (the dead `Fallible` trait is deleted); the
same stop report with a source arrow on every tier, web included; contracts
that actually run under `jet run` and erase when the type system proves them;
an exit-code law that separates reported errors (1), stops (70), and Jet's
own defects (101); and a spelling to catch the error value in a fallback
instead of swallowing it.

Seven ballots (D-FAIL-MODEL1 … D-FAIL-BIND1) ask direction-level questions.
Each stands alone. What does not change: `T ? E` and `?`/`??` spellings, the
optional/fallible whitespace canon, trap-on-overflow for sized widths,
validate-block accumulation, `#Transact` rollback, the no-exceptions wall,
and cancellation's one unwind engine. Task failure at join stays in the
concurrency slate's lane (its proposed D-CONC-FAIL1 row, not yet a minted
ballot); this proposal supplies the value it rides on.

## Glossary

- **Report** — the one failure product: a registered code, a message in plain
  words, a why, a fix, a source location, and an optional cause chain. A
  compile diagnostic is a report at compile time. A runtime failure is the
  same report at run time. Routes carry different subsets: a value carries
  message, code, and cause; the why, fix, and location dress it when a
  boundary renders it. A stop always renders the full report.
- **Route** — how a report reaches someone. Three exist: *value* (returned in
  the signature), *attributed stop* (the program stops; the report names the
  broken promise), *contained stop* (a boundary limits the blast and keeps
  the report).
- **Attribution** — whose promise broke: the **world** (input, files, the
  network), **code** (a promise written in the program), or the **substrate**
  (the compiler, the runtime, a foreign library).
- **Boundary** — a spelled place where a route may change: a task join, a
  test body, the process edge, an FFI edge, a `#Transact` region.
- **Breach** — a code-attributed failure: a broken contract, an overflow, an
  index out of range, a `require` that failed, a `panic(...)` call.
- **Fault** — a substrate-attributed failure: a foreign crash, a broken
  runtime invariant, or Jet's own defect. A fault inside the program's run is
  contained and exits 70. Jet's own defect is reported as an internal
  compiler error and exits 101 — Jet's report about itself, outside the
  program's routes.
- **Erasure by proof** — a check the compiler can prove never fails is
  removed; only unproven checks run. Same rule the type system v2 uses for
  knowledge (D-TYPE2-EXACT1).

## The one idea

**Every failure is one report; attribution picks one of three routes; routes
change only at spelled boundaries.**

The beginner story: you never learn three error systems. You learn `?` to
pass a failure up, `??` to give a fallback, and that a stop prints the same
kind of friendly report the compiler prints — with a code you can ask
`jet explain` about. The report looks the same whether the compiler caught it
before the run or the program hit it during the run.

The expert story: you get the full algebra. Errors are real values with
codes and cause chains. You mint your own error families and declare
conversions on one rail. Contracts run on every tier and erase when the
range facts prove them. Boundaries are explicit and auditable: you know
exactly where a stop becomes a value, and nothing swallows a failure without
a spelled reason.

## Evidence: the shadow systems

Seventeen mechanisms deliver failures today. Each has its own home and its
own defect.

| # | Mechanism | Home | Defect |
|---|---|---|---|
| 1 | `T ? E` + default `Error` | `crates/jet-codegen/src/Codegen/Context.rs:1325` | `Error` lowers to plain `String`; no fields, no matching, no chain |
| 2 | `Fallible` trait / `to_error` | `crates/jet-foundation/src/Syntax/effects_surface.rs:337` | not a builtin, not synthetic, zero implementations possible; E2402's fix text is itself a hard error (E0321) |
| 3 | Declared conversion `impl S => T` | `crates/jet-foundation/src/Traits.rs:1250-1300` | works; one of two rails for one job |
| 4 | `.context("msg")` | `crates/jet-sema/src/Sema/CheckerInfer/calls/method_calls.rs:2035-2057` | only when the error type is `Error`; lowers through a `"__Fallible__"` string sentinel; chain is string concat |
| 5 | `#Pre` / `#Post` | `crates/jet-codegen/src/Codegen/Items.rs:2277-2344` | AOT-only; `jet run` silently skips them; arrow points at the marker, not the call |
| 6 | `require` / `panic` | `crates/jet-codegen/src/Prelude/Core.rs:1432` (`jet_panic`), `:1568` (`jet_panic_rich`) | separate renderer from contracts (`jet_contract_fail`, `:1463`) |
| 7 | Arithmetic and bounds traps | `crates/jet-codegen/src/Prelude/Core.rs:1500-1562` | wording differs AOT vs JIT; JIT drops the `--> file:line`; `?? panic("msg")` prints `panic: panic` under JIT |
| 8 | Comptime failures (E0953) | `Source/Interpreter.rs:157-172`, `crates/jet-jit/src/jit/deopt.rs:320-337` | diagnostic rewritten into a runtime stop in three places |
| 9 | `#Todo` / proved-unreachable arms | `crates/jet-codegen/src/Codegen/TIR/emit/expressions.rs:2566-2573` | raw Rust panic; exit 101 collides with the ICE code (I2) |
| 10 | Raw `panic!` in the Prelude | `crates/jet-codegen/src/Prelude/LocalCell.rs:115` and 10+ more | exit 101, no Jet location; the `Cell borrow conflict` panic the spec promises is one of them |
| 11 | Entry-point error exit | `crates/jet-codegen/src/Codegen/Items.rs:810-854` | two frames: bare string + exit 1 vs `CryptoError` full frame + exit 70 (101 on Internal); every other typed error refused at the entry (E0122) |
| 12 | Scheduler fatal | `crates/jet-codegen/src/Prelude/Scheduler.rs:111,2301` | direct `process::exit(70)`, its own carrier, doubled message in goldens |
| 13 | FFI foreign panic | `crates/jet-pkg-model/src/FFI.rs:2795` | direct `exit(70)`; skips every cleanup |
| 14 | `task.exception()` | `docs/spec/spec.md:2159` | cancellation state smuggled as a bare `String`; the concurrency slate proposes deleting it |
| 15 | `validate` / `[FieldError]` | `crates/jet-sema/src/Sema/CheckerValidate.rs` | healthy value-route island; ratified rule vocabulary (D-VALIDATE3) and `Validate.over` never built |
| 16 | Runtime diagnostics E3001/E3002/E3003/E3005 | `docs/spec/diagnostics.md:465-468` | 4 runtime codes vs 1064 compile codes; four renderers plus a fifth that rebuilds diagnostics from runtime strings (`Source/CmdProve.rs:2744-2790`) |
| 17 | Web-tier failure | `crates/jet-codegen/src/Codegen/Web.rs:4854-4863, 4913-4968` | wasm tier stops with a raw panic and no report; JS tier throws bare `Error(msg)`; no code, no parity (I9) |

Supporting counts: 209 `Result<_, String>` returns and 95 `Err(format!(...))`
sites in the shipped Prelude; five closed `*ErrorKind` tables users cannot
extend. Runtime `.err.out` behavior is not a tracked parity dimension: the
dev-backend ledger names exactly one expected stop
(`tests/dev.rs:657-659`).

## The model

Three axes, one law.

**Axis 1 — attribution: world / code / substrate.** Whose promise broke.
This is the owner's question, and it is the deciding axis.

**Axis 2 — route: value / attributed stop / contained stop.** Attribution
picks the route. World → value in the signature. Code → attributed stop.
Substrate → contained stop.

**Axis 3 — moment: compile time / run time.** A promise the compiler can
prove is checked at compile time or erased. An unproven promise is checked at
run time. The report is the same product at either moment.

**The law: no failure is lost, reworded, or rerouted silently. One report,
delivered exactly once, on the route its attribution picks. A route changes
only at a spelled boundary.**

What each route owes:

- A **value** owes the caller a typed, matchable report with a cause chain,
  presence in the signature, and intact callee state — so the caller can
  retry or continue.
- An **attributed stop** owes exactly one thing: attribution. Which promise,
  which party, which site. It owes no catch, because after a broken code
  promise every invariant is suspect; running handler code would act on state
  the program already proved it does not understand.
- A **contained stop** owes containment: the blast radius equals the
  boundary, and the report survives the boundary's death.

Ratified law, re-read as theorems of this model:

- E0401/E0402/E0419 (must-use) and `.drop("reason")` as the sole discard —
  "delivered exactly once" on the value route (D-IGNORERET1/2,
  D-MARK-DISCARD1).
- I2 / exit 101 — substrate attribution: the compiler's own failures never
  wear a program's report.
- The exit table (S36) — the process edge is a boundary; the exit code is the
  value the operating system receives.
- D-VALIDATE1's three-layer ruling — the layers are three attributions, not
  three competing checkers. Proven facts live in types and erase. Outside
  input is world-attributed and accumulates on the value route as
  `[FieldError]`. Code promises are breaches and stop. The ruling that they
  do not compose was correct — they were never the same job.
- D-INTBIG1 (whole numbers never overflow) — the best failure model deletes
  failure cases. D-NUMOPS1 (sized widths trap) — a sized op's range is a code
  promise; breaking it is a breach. `U8.from_int(n)?` — narrowing outside
  data is world-attributed, so it is a value. Three outcomes, one rule.
- D-CANCELMODEL1=C — one unwind engine is the containment machinery.
  Cancellation is not a failure; it is a boundary event that rides the same
  engine and surfaces as a value at the join.
- I9 — one report on every tier is the parity invariant applied to failure.

The "ohhh" connections:

1. A runtime failure and a compile diagnostic are the same product at
   different moments. `jet prove` already rebuilds diagnostics from runtime
   strings by hand — the fragments were converging on this without a name.
2. A trap is a breach of a built-in contract. Overflow, divide-by-zero, and
   out-of-bounds are `#Pre` conditions Jet wrote for you on `+`, `/`, and
   `[]`. One family, one renderer, one exit.
3. The `Fallible` trait is a shadow copy of declared conversion — and it
   never worked. `impl MyErr => Error` on the one rail replaces it exactly.
4. A boundary turns a stop into a value. A task join returns
   `.Err(.Panicked)`. A test's `.expect_fail` consumes a stop. The process
   edge turns an unhandled error into exit 1. These are one conversion law,
   not three features.
5. D-VALIDATE1's "three layers do not compose" is the model's attribution
   axis stated early. Types erase, input accumulates, promises stop.
6. `#Transact` rollback hooks are error-path cleanup — the thing Zig calls
   `errdefer`. The parked proposal (card #775) worries about a mechanism
   that partly shipped while it waited.
7. The exit code is `run`'s error type as the operating system reads it.

## The surface

The heart of the proposal. Before/after pairs from real programs. Each item
is marked ratified, amended, or new.

### 1. `Error` becomes a real value — amended (S80/D-ERR2 spelling)

S80 ratified a structured `Error` (message, optional code, optional source)
with a builder spelling (`Error.message("…").code(n).with_source(e)`). None
of it was built. The amended spelling is one constructor with labels, per the
corelib doctrine's own option rule (D4: options are labels, not chains).

Before — today the only way to make an `Error` is a bare string:

```jet
fn load_config(path: String) => Config ? Error {
    text :: fs.read(path)?               // error is a bare string
    cfg :: parse(text) ?? return Err("parse failed")   // prose, no cause
    return Ok(cfg)
}
```

After — proposed:

```jet
fn load_config(path: String) => Config ? Error {
    text :: fs.read(path).context("reading config at {path}")?
    cfg :: parse(text).context("parsing {path}")?
    return Ok(cfg)
}
// On failure the caller holds a real value:
//   e.message  -> "parsing app.toml"
//   e.cause    -> the parser's own report
//   e.code     -> the registered code, when one exists
// Unhandled at the process edge it prints the full chain:
//   Error: parsing app.toml
//     cause: unexpected token at line 3
```

`Error("msg")`, `Error("msg", code: "CFG404")`, and `Error("msg", cause: e)`
are the constructors; `code` is a short string, so registered codes and app
codes ride one field. `Err("msg")` keeps working as sugar for
`Err(Error("msg"))`. `.context` works on every fallible value, whatever its
error type — it wraps the old error as the cause. D-ERRCTX1 never scoped
`.context` to `Error`; that limit is an implementation artifact, so widening
it is a fix, not an amendment. D-ERRCTX1's other half, the dev-build `?`
trace (E3002), stays.

### 2. One conversion rail — amended (deletes D-ERR2's `Fallible` clause)

Before — the diagnostic teaches a dead end:

```jet
enum StoreErr { Missing, Locked }

fn get_user() => User ? Error {
    return read_store()?
    // Error [E2402]: `StoreErr` has no path to `Error`
    //  Fix: add `impl StoreErr: Fallible { fn to_error(self) => Error { … } }`
    //  — that spelling is E0321, the trait does not exist, and no
    //    expression produces an `Error`. There is no way out.
}
```

After — proposed: the `Fallible` trait and `to_error` are deleted. Error
conversion has exactly one mechanism, the already-ratified declared
conversion (D-ERR-CONV), now usable with `Error` as the target:

```jet
impl StoreErr => Error { return Error("the store is unavailable: {self}") }

fn get_user() => User ? Error {
    return read_store()?      // converts on the one rail, chain kept
}
```

E2402's text is rewritten to teach this. The `TryConvert::Fallible` sema arm
and the trait constants are removed. One mechanism (I8), and it is the one
that already works.

One rule bends to make this whole: the orphan rule (E2406) keeps its meaning
for typed targets, but a conversion into core `Error` may name a foreign
source type. Today E2406's registered fix text sends people who own neither
type to the dead trait; with the trait gone, the `Error`-target carve-out is
the working escape. E2402, E2406, D-LIB3's `?`-conversion registration, and
the `Fallible` block in `docs/reference/syntax-surface.jet` are all amended
in the same change.

### 3. Every stop is a report — new (implements I4 at run time)

Before — four renderers, tier-divergent wording, lost locations:

```text
$ jet build app.jet && ./build/app
panic: the list has 3 items, so position 10 doesn't exist
  --> app.jet:12

$ jet run app.jet
panic: index out of bounds: the index is outside the list      # no location

$ jet run port.jet          # n :: load() ?? panic("could not load the port")
panic: panic                                                   # message lost
```

After — proposed: one renderer in the Prelude (I9: engines marshal, the
Prelude owns meaning). Every stop — trap, contract, `require`, `panic`,
`#Todo` — carries a registered code in the E30xx runtime family, the same
what/why/fix voice as compile diagnostics, and the source arrow, on every
tier:

```text
$ jet run app.jet        # identical under jet build, dev, and the interpreter
Stop [E3010]: the list has 3 items, so position 10 doesn't exist
  --> app.jet:12 in run
 Why: reading past the end of a list has no answer to give.
 Fix: check the position first, or use `list.get(i)` for a maybe-value.
```

`jet explain E3010` works like any compile code. `.expect_fail` in tests can
name the code it expects. The web tier throws carry the same report text
(exit codes do not exist there; the report does). The ratified trap message
strings from the math slate (for example "divided by zero", D-FLOORDIV1) are
kept word-for-word as the `Stop` message line. E3001's registered rich frame
— the source-line box and the debug-build safe locals (D-OBS1/D-OBS2) — is
kept; the samples here are abbreviated.

### 4. Contracts run everywhere and erase under proof — amended (D-PREPOST1: tier delivery plus a proof disposition)

Before — ratified text says "checked in every build"; reality:

```text
$ jet run fee.jet          #  #Pre(cents > 0, "cents must be positive")
total: -95                 # exit 0 — the contract never ran

$ jet build fee.jet && ./build/fee
#Pre contract failed: cents must be positive
  --> fee.jet:1            # arrow points at the marker, not the call
```

After — proposed: contracts get a TIR node and a Prelude check, so AOT, JIT,
and the interpreter all run them (I9). The report blames the right party at
the right site: a `#Pre` breach points at the **call site** (the caller broke
the promise); a `#Post` or `require` breach points at the **body** (the
callee broke its own). And when the type system already proves the condition,
the check erases:

```jet
#Pre(cents > 0, "cents must be positive")
fn add_fee(cents: Int) => Int { ... }

fn run() {
    add_fee(-100)
    // Stop [E3005]: #Pre contract failed: cents must be positive
    //   --> app.jet:4 in run        (the call site, on every tier)
}

fn charge(cents: Int(1..)) {
    add_fee(cents)     // proven by the range fact — no runtime check emitted
}
```

This is the same move D-TYPE2-REFINE1 already made for `#Invariant`: proof
first, check as the fallback. The three constraint layers now compose by
attribution instead of colliding.

Named amendment: D-PREPOST1 ratified "checked in every build" with one
explicit opt-out, the per-module build-policy strip. Erasure-under-proof is
a third disposition — proven, so not emitted — and this ballot names it as
an amendment to that clause, not a delivery detail. The explicit strip
opt-out stays as ratified.

### 5. Exit codes get an honest law — amended (S36, the architecture exit table, and the entry rule)

Before — three behaviors from one position:

```text
fn run() => () ?              → bare string on stderr, exit 1, no report frame
fn run() => () ? CryptoError  → full E3001 frame, exit 70 (or 101 on Internal)
fn run() => () ? StoreErr     → E0122: not allowed at the entry point
```

After — proposed: any error type with a conversion to `Error` may leave
`run`. An unhandled error at the process edge is a value delivered to the
operating system: the report prints with its frame and cause chain, and the
process exits **1** ("the work failed"). A breach or program-side fault
exits **70** ("the program broke"). Exit **101** belongs to Jet's own
defects alone — `#Todo` moves to a proper E30xx stop at 70, the raw Prelude
panics are converted to reports, and the `CryptoError` special case is
deleted. Named amendments: the architecture exit table's producer column
(70 widens from panic/require to every stop; 1 gains the runtime-reported
error), S36's producer note, and E3001's registered entry text (its
"Internal exits 101" sentence).

### 6. The fallback can hold the error — new

Before — `??` must swallow:

```jet
port :: read_port() ?? 8080          // why did it fail? gone.
if result == { .Err(e) -> print("error") }   // e discarded in practice
```

After — proposed spelling (ballot offers alternatives):

```jet
port :: read_port() ?? (e) => { log.warn("using default port: {e}") ; return 8080 }
```

The bound form keeps `??`'s one job — produce the fallback — while letting
the handler see the report. A `return` inside the bound form returns the
fallback value, exactly as in any small function. `?? value`, `?? return`,
`?? panic(...)`, `?? break`, `?? next` all stay, and so does the ratified
`?? (next)` value form (D-ORRETURN-CANON1): the binder is recognized only
by the `=>` after the closing parenthesis.

### 7. Deletions

- The `Fallible` trait, `to_error`, and the `TryConvert::Fallible` arm.
- Three of the four runtime renderers and the `CmdProve` reconstructor; one
  Prelude renderer remains.
- The `JetParaRuntimeFailure` side-carrier and the direct
  `process::exit(70)` sites in the scheduler, streams, and FFI (they route
  through the one boundary, so cleanup runs).
- The `CryptoError` entry special case and its 101 path.
- The dead `if false &&` teaching branches in the parser (eleven sites) —
  resurrected as real teaching errors with current fix text, or removed with
  their constants.
- 209 `Result<_, String>` Prelude returns migrate to typed families under
  the corelib doctrine (F1-F3) as modules are touched — a cleanup stream,
  not a flag day.

## What it looks like

One program, three levels. Today's code on the left of each pair; proposed
lines are marked.

**Beginner — a tool that reads a file and reports failure well:**

```jet
// today
fn run() => () ? {
    text :: fs.read("notes.txt")?        // works, but the error is a string
    print(text)
}

// proposed: same shape, richer failure — no new ceremony
fn run() => () ? {
    text :: fs.read("notes.txt").context("loading your notes")?   // proposed
    print(text)
}
// $ jet run notes.jet      (file missing)
// Error: loading your notes
//   cause: no file at notes.txt
// exit 1
```

**Middle — a service with a typed error family:**

```jet
enum ApiErr { NotFound, RateLimited, Upstream(Error) }

impl IOError => ApiErr { return ApiErr.Upstream(Error("io: {self}")) }  // proposed target spelling
impl Error => ApiErr { return ApiErr.Upstream(self) }                   // proposed

fn fetch(id: Int) => Record ? ApiErr {
    raw :: store.read(id)?               // IOError converts on the one rail
    rec :: decode(raw)?                  // Error converts to ApiErr.Upstream
    return Ok(rec)
}

fn handle(id: Int) => Response {
    if fetch(id) == {
        .Ok(r)                -> return respond(r)
        .Err(.NotFound)       -> return status(404)
        .Err(.RateLimited)    -> return status(429)
        .Err(.Upstream(e))    -> { log.error("{e}") ; return status(502) }
    }
}
```

**Expert — contracts, proof, a boundary, and rollback:**

```jet
#Pre(amount > 0, "amount must be positive")
fn debit(account: &Account, amount: Int) { ... }

fn settle(batch: [Transfer]) => () ? Error {
    #Transact(t) {
        loop tr, batch {
            debit(&accounts[tr.from], tr.amount)   // proven when amount: Int(1..)
            credit(&accounts[tr.to], tr.amount)
        }
    }                                              // undo runs if we leave by failure
    return Ok(())
}

fn run() => () ? {
    result :: task settle(load_batch()?)           // concurrency slate spelling
    result.join()?                                 // a panicked task arrives as a value
    return Ok(())                    // (the concurrency slate's proposed D-CONC-FAIL1 lane)
}
```

## What this unlocks

- **Libraries.** Authors mint error families and one conversion each; cause
  chains survive across crates. The gap Rust left to third-party crates
  (context, chaining, derive-style authoring) is closed in core, which is
  the last moment it can be — retrofits of error identity are the one thing
  peers never recovered from.
- **Applications.** One `Error` with context everywhere; the beginner's
  `? Error` program and the expert's typed-family program are the same
  program at two zoom levels.
- **Tests.** `.expect_fail(E3010)` (code proposed) asserts *which* failure, not just
  "something stopped".
- **Tooling.** `jet explain` covers runtime stops; the parity ledger tracks
  `.err.out` examples on every tier, so runtime failure becomes a tested
  dimension instead of a silent gap.
- **Concurrency and services.** The join boundary (the concurrency slate's
  proposed D-CONC-FAIL1), restart rules, and supervision all ride one value
  shape. That slate deletes the string-typed `task.exception()`; this model
  supplies the typed value that replaces it.
- **Critical software.** Contracts that erase under proof are the SPARK
  direction: one annotation, proof where possible, a check where not, a
  breach report where the check fires. The model has the slot; the proof
  engine can grow into it.
- **The web tier.** One report voice on native and web ends the bare
  `throw new Error` divergence and makes I9 honest for failure.

## What stays

- `T ? E`, `?`, `??`, and the whitespace canon `T?` vs `T ? E`
  (D-RESULT-OPTION-CANON1) — earned on merit; nothing reads better.
- Trap-on-overflow for sized widths with `wrapping`/`saturating`/`checked`
  escapes (D-NUMOPS1, D-INTBIG1) — correct attribution, kept word-for-word.
- `validate` blocks accumulating `[FieldError]` (D-VALIDATE1,
  D-VALIDATE-DECODE1) — the value route for outside input; the ratified
  rule vocabulary (D-VALIDATE3) and `Validate.over` finally get built on
  this substrate.
- `#Transact`, the `Rollback` trait, and E0746 (D-TXN1-4, D-STM1) — the
  error-path cleanup boundary. Card #775 stays frozen per the owner's
  instruction; this proposal notes its subject partly shipped.
- The dev-build `?`-propagation trace (E3002, D-ERRCTX1) — kept as is.
- `panic` / `require` (S36) and the exit table — with single producers.
- The no-exceptions wall: no `try`/`catch`, no in-process recovery of stops.
  Boundaries are the only converters.
- Cancellation as a boundary event on one unwind engine (D-CANCELMODEL1=C),
  surfacing as a value at the join — the concurrency slate's lane.
- `.drop("reason")` as the sole spelled discard (D-MARK-DISCARD1).

## Decisions for the owner

| Ballot | Question | Recommends |
|---|---|---|
| D-FAIL-MODEL1 | Adopt "one report, three routes" as the failure law? | adopt |
| D-FAIL-ERROR1 | `Error` becomes a real value — constructor spelling? | `Error("msg", code:, cause:)` labels |
| D-FAIL-CONV1 | Delete the `Fallible` trait; one conversion rail? | delete it |
| D-FAIL-BREACH1 | Every stop is a registered report on every tier? | yes, one renderer |
| D-FAIL-TIER1 | Contracts on every tier, erased under proof, blame at the right site? | yes |
| D-FAIL-EXIT1 | Exit-code law: report+1 for errors, 70 for stops, 101 compiler-only? | yes |
| D-FAIL-BIND1 | Spelling for catching the error in a fallback? | `?? (e) => expr` |

Each ballot stands alone; any subset can be adopted. Ratified decisions each
one amends are named inside the ballot text.

## Implementation shape

**Phase A — internal re-founding, no surface change.** One report carrier
and one renderer in the Prelude; every existing message string preserved
exactly; the scheduler, stream, FFI, and `JetParaRuntimeFailure` paths route
through it, and the para deferred-failure aggregation keeps its behavior on
the one carrier; TIR nodes for contracts; the parity ledger gains the
`.err.out` dimension. All goldens byte-identical except the defect cases (JIT wording,
lost spans) which get their own cards.

**Phase B — land the ratified-but-unbuilt on the substrate, built once.**
Structured `Error` (S80 as amended), universal `.context` (D-ERRCTX1 as
amended), the D-VALIDATE3 rule vocabulary, `Validate.over`, the E2402
retext, entry-point relaxation, `#Todo` to a real stop.

**Phase C — balloted surface unifications, each a coherent greenfield
migration.** The conversion-rail deletion, the breach code family with web
parity, the exit law, the fallback binding. Each deletes its replaced form
in the same change (greenfield rule; no aliases, no fallbacks).
