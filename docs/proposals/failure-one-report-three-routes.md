# Failure: one report, three routes

Status: settled law, 2026-08-06. All eleven D-FAIL-* decisions on card #1507
are ratified. This document records the final proposal and slate.

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

The deeper unification sits under the spellings. `T?` and `T ? E` are two
views of one carrier. An outcome has a payload (a value, part of one, or
none), a verdict (clean, succeeded with notes, or failed), and attached
reports. Absence is the optional view. Failure is the fallible view.
Success-with-a-warning and failure-with-partial-results are the same
carrier's middle states — no third type needed. The everyday spellings
stay; they gain depth instead of neighbors.

The beginner surface gets more magic, not less. `fn run()` is fallible by
default; nobody types `() ? Error` to use `?` in a first program. A bare
`? E` return clause means "returns nothing, can fail". Context rides the
`?` itself — `fs.read(path)? "loading config"` — because the `?` is where
the error path already lives on the page. Every hop a failure takes joins
one automatic journey, and the final report prints the whole story. Inside
a `??` fallback, `err` names the failure with no lambda ceremony. Experts
keep every dial: typed families, declared conversions, pinned entry types,
proof-erased contracts, and audited boundaries.

What the owner gets on the page: a real default error built with
`Err("msg")`; one conversion rail instead of two (the dead `Fallible` trait
is deleted); the same stop report with a source arrow on every tier, web
included; contracts that actually run under `jet run` and erase when the
type system proves them; an exit-code law that separates reported errors
(1), stops (70), and Jet's own defects (101); and a program edge that
delivers unhandled errors in the target's native shape — stderr and an exit
code for a CLI, a typed error object for a web app, a host-visible value
for a wasm module.

Eleven ratified decisions (D-FAIL-*) settle the failure law. What does not
change: `T ? E` and `?`/`??` spellings, the
optional/fallible whitespace canon, trap-on-overflow for sized widths,
validate-block accumulation, `#Transact` rollback, the no-exceptions wall,
and cancellation's one unwind engine. Task failure at join stays in the
concurrency slate's lane; this proposal supplies the value it rides on.

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
- **Carrier (outcome)** — the one type under `T?` and `T ? E`: a payload (a
  value, part of one, or none), a verdict (clean, noted, or failed), and
  attached reports.
- **Note** — one short context line attached to a report as it travels.
- **Journey** — the ordered list of hops and notes a failure collected on
  its way up; the final report prints it.

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

## The carrier: one type under `T?` and `T ? E`

Today Optional and Result are separate types with separate rules, and
nothing bridges them but `?` and `??` guessing from the return type. The
reconstruction: both are views of one carrier with three independent facts.

- **Payload** — a value, part of a value, or none.
- **Verdict** — clean, succeeded with notes, or failed.
- **Reports** — the notes and the failure report, with cause and journey.

The familiar cases are corners of this grid. Clean value: plain `T`.
Absence: no payload, clean verdict — that is `T?`, and it is why absence is
not an error. Failure: no payload, failed verdict with a report — that is
`T ? E`. The middle states exist in real programs and today have no home:

```jet
// success with a note: the value is good, but something is worth saying
resp :: fetch(url)?                 // resp carries "retried twice" as a note
// notes ride the journey; nothing to unwrap, nothing extra to learn

// failure with partial results: 90 of 100 rows imported
rows :: import_rows(file) ?? { save(err.partial ?? []) ; return report(err) }
```

The views convert without ceremony because they are the same carrier:
`.or_err("why")` turns an absence
into a failure with a report; a failure read where an optional is wanted
collapses to absence and logs its report to the journey. The everyday
spellings `T?` and `T ? E` stay exactly as ratified
(D-RESULT-OPTION-CANON1) — they are the two views people reach for, not two
types to reconcile.

This is the same shape the type system v2 gave numbers: one carrier,
knowledge layered on top. Verdict and notes are knowledge about an outcome;
they erase from the happy path and cost nothing when unused. D-FAIL-CARRIER1=A
settles this carrier.

## The surface

The heart of the settled law. Before/after pairs from real programs. Each
item names its amendment or final decision.

### 1. The default error is `Err`, a real value (D-FAIL-ERROR1=A; amends S80 and D-ERR2)

S80 ratified a structured default error (message, optional code, optional
source) with a builder spelling (`Error.message("…").code(n).with_source(e)`).
None of it was built, and the shipped type is a plain string. The amended
form: one word, `Err`, is both the constructor and the type name. There is
no second spelling to learn — the `Err("msg")` beginners already write *is*
the constructor.

Before — today the only way to make a default error is a bare string:

```jet
cfg :: parse(text) ?? return Err("parse failed")   // prose, no cause, no fields
```

After:

```jet
return Err("parse failed")                      // same line, now a real value
return Err("config rejected", code: "CFG404")   // labels add the rest
return Err("parse failed", cause: e)            // chains keep the old report
// The caller holds a value: e.message, e.code, e.cause — matchable, printable.
// Unhandled at the program edge it prints the full chain:
//   Error: parse failed
//     cause: unexpected token at line 3
```

`code` is a short string, so registered codes and app codes ride one field.
In signatures the type is written `Err` (`Config ? Err`), and mostly it is
not written at all: bare `T ?` already implies it (S34, amended).

### 2. Context rides the `?` itself (D-FAIL-CTX1=A; amends D-ERRCTX1)

The `?` is where the error path already lives on the page, so that is where
context belongs — not a method call dangling off the end of the line.

Before — context is a trailing method, and only for the default error type:

```jet
text :: fs.read(path).context("reading config at {path}")?
```

After: a note is a string written after the `?`:

```jet
fn load_config(path: String) => Config ? {
    text :: fs.read(path)? "reading config at {path}"
    cfg :: parse(text)? "parsing {path}"
    return Ok(cfg)
}
```

The note attaches to that hop of the failure's journey, lazily interpolated
(D-ERRCTX1's rule, kept). It works on every fallible value, whatever its
error type. Notes are optional: a bare `?` still joins the journey
automatically, so an unhandled failure prints where it traveled even when
nobody wrote a word. This is the automatic-context propagation the owner
asked for: Zig's error return traces made a product, plus human notes where
they help. The `.context` method is deleted. The journey renders on every
tier.

### 3. One conversion rail (D-FAIL-CONV1=A; amends D-ERR2 and D-LIB3)

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

After: the `Fallible` trait and `to_error` are deleted. Error
conversion has exactly one mechanism, the already-ratified declared
conversion (D-ERR-CONV), now usable with the default error as the target:

```jet
impl StoreErr => Err { return Err("the store is unavailable: {self}") }

fn get_user() => User ? {
    return read_store()?      // converts on the one rail, chain kept
}
```

A word on the spelling, because it reads unusually: `impl Source => Target`
is not new here — it is the ratified, shipped declared-conversion form
(D-ERR-CONV, `examples/features/errors/typed_error_families.jet`). It is
deliberately lambda-shaped: a conversion *is* a function from source to
target, written once, applied by `?` wherever the types demand it. `self`
is the source value; the body returns the target. This spelling is final.

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

### 4. Every stop is a report (D-FAIL-BREACH1=A; implements I4 at run time)

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

After: one renderer in the Prelude (I9: engines marshal, the
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

### 5. Contracts run everywhere and erase under proof (D-FAIL-TIER1=A; amends D-PREPOST1)

Before — ratified text says "checked in every build"; reality:

```text
$ jet run fee.jet          #  #Pre(cents > 0, "cents must be positive")
total: -95                 # exit 0 — the contract never ran

$ jet build fee.jet && ./build/fee
#Pre contract failed: cents must be positive
  --> fee.jet:1            # arrow points at the marker, not the call
```

After: contracts get a TIR node and a Prelude check, so AOT, JIT,
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
a third disposition — proven, so not emitted — and D-FAIL-TIER1 names it as
an amendment to that clause, not a delivery detail. The explicit strip
opt-out stays as ratified.

### 6. The entry is fallible by default, and unit-fallible signatures lose their clutter (D-FAIL-EXIT1=A; D-FAIL-UNIT1=A)

Before — a beginner's first `?` forces ceremony, and the ceremony is ugly:

```text
fn run() => () ?              → the beginner types "() ?" to read a file
fn run() => () ? CryptoError  → full E3001 frame, exit 70 (or 101 on Internal)
fn run() => () ? StoreErr     → E0122: not allowed at the entry point
```

After: `fn run()` is fallible by nature, because programs are.
No annotation, ever, for the default case:

```jet
fn run() {
    text :: fs.read("notes.txt")? "loading your notes"
    print(text)
}
// $ jet run notes.jet      (file missing)
// Error: loading your notes
//   cause: no file at notes.txt
// exit 1
```

An expert pins the family like any other narrowing, and every unit-fallible
function drops the `()` — a bare `?` clause means "returns nothing, can
fail":

```jet
fn run() ? StoreErr { ... }          // pinned entry family
fn save(path: String) ? IOError { ... }   // no arrow, no unit
```

The exit law underneath: an unhandled error at the process edge is a value
delivered to the operating system — the report prints with its frame and
journey, and the process exits **1** ("the work failed"). A breach or
program-side fault exits **70** ("the program broke"). Exit **101** belongs
to Jet's own defects alone — `#Todo` moves to a proper E30xx stop at 70,
the raw Prelude panics are converted to reports, and the `CryptoError`
special case is deleted. Named amendments: S80's entry clause (implicit
fallible `run`; E0122's list dies), S34's bare-`?` rule extended to the
whole return clause, the architecture exit table's producer column, S36's
producer note, and E3001's registered entry text. D-FAIL-EXIT1 and
D-FAIL-UNIT1 settle these amendments.

### 7. The fallback can see the error, with no lambda (D-FAIL-BIND1=A)

Before — `??` must swallow, or the code grows an eight-line arm table:

```jet
port :: read_port() ?? 8080          // why did it fail? gone.
if result == { .Err(e) -> print("error") }   // e discarded in practice
```

After: inside a `??` fallback, `err` simply names the failure:

```jet
port :: read_port() ?? { warn("using default port: {err}") ; return 8080 }
```

No lambda, no binder syntax, no parentheses. `err` is in scope only inside
the fallback expression, the way `result` is in scope only inside `#Post`.
On an optional's fallback there is no failure to name, so `err` there is a
compile error. `?? value`, `?? return`, `?? panic(...)`, `?? break`,
`?? next` all stay, and the ratified `?? (next)` value form
(D-ORRETURN-CANON1) is untouched — no new binder grammar exists to collide
with it.

### 8. The program edge adapts to the target (D-FAIL-EDGE1=A)

One program shape, many delivery shapes. Today the edge is native-only:
report text and an exit code, emulated poorly or not at all elsewhere.

The edge boundary converts an unhandled error into the target's
native failure shape, carrying the same report:

```text
CLI / native binary   → report frame on stderr, exit 1
web app               → a typed error object on the page/console, report inside
wasm module           → a host-visible error value, report inside
service               → a structured report to the log and the supervisor
test                  → a test failure naming the report
```

The report is the constant; the delivery is target-native. This is the
boundary law applied to the last boundary, and it replaces today's bare
`throw new Error(msg)` divergence with design instead of emulation.
D-FAIL-BREACH1 owns the report's words; D-FAIL-EDGE1 owns its delivery.

### 9. Deletions

- The `Fallible` trait, `to_error`, and the `TryConvert::Fallible` arm.
- The `.context` method — its job moves to the `?` note, one operator
  instead of an operator plus a method.
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

One program, three levels. Current code is on the left of each pair. Settled
lines follow.

**Beginner — a tool that reads a file and reports failure well:**

```jet
// today — the beginner must spell "() ?" before their first ? works
fn run() => () ? {
    text :: fs.read("notes.txt")?        // works, but the error is a string
    print(text)
}

// settled: zero ceremony — run is fallible by nature
fn run() {
    text :: fs.read("notes.txt")? "loading your notes"   // note on the ?
    print(text)
}
// $ jet run notes.jet      (file missing)
// Error: loading your notes
//   cause: no file at notes.txt
// exit 1
```

**Middle — a service with a typed error family:**

```jet
enum ApiErr { NotFound, RateLimited, Upstream(Err) }

impl IOError => ApiErr { return ApiErr.Upstream(Err("io: {self}")) }
impl Err => ApiErr { return ApiErr.Upstream(self) }

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

fn settle(batch: [Transfer]) ? Err {               // no arrow, no unit
    #Transact(t) {
        loop tr, batch {
            debit(&accounts[tr.from], tr.amount)   // proven when amount: Int(1..)
            credit(&accounts[tr.to], tr.amount)
        }
    }                                              // undo runs if we leave by failure
    return Ok(())
}

fn run() {
    result :: task settle(load_batch()?)           // concurrency slate spelling
    result.join()? "settling the day's batch"      // a panicked task arrives as a value
}                                    // separate concurrency slate lane
```

## What this unlocks

- **Libraries.** Authors mint error families and one conversion each; cause
  chains survive across crates. The gap Rust left to third-party crates
  (context, chaining, derive-style authoring) is closed in core, which is
  the last moment it can be — retrofits of error identity are the one thing
  peers never recovered from.
- **Applications.** One default error with context everywhere; the
  beginner's bare program and the expert's typed-family program are the
  same program at two zoom levels.
- **The middle states.** Success-with-notes and failure-with-partial-results
  get a home: batch jobs, imports, degraded fetches, and best-effort
  pipelines stop inventing side channels for "it mostly worked".
- **Tests.** `.expect_fail(E3010)` asserts *which* failure, not just
  "something stopped".
- **Tooling.** `jet explain` covers runtime stops; the parity ledger tracks
  `.err.out` examples on every tier, so runtime failure becomes a tested
  dimension instead of a silent gap.
- **Concurrency and services.** The join boundary (the concurrency slate's
  concurrency failure decision), restart rules, and supervision all ride one value
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

## Settled decisions (2026-08-06)

| Decision | Settled outcome |
|---|---|
| D-FAIL-MODEL1 | A — one report, three routes; attribution picks the route |
| D-FAIL-CARRIER1 | A — one carrier under `T?` and `T ? E`, with payload, verdict, and reports |
| D-FAIL-ERROR1 | A — `Err` is the default error type and constructor; amends S80 and D-ERR2 |
| D-FAIL-CONV1 | A — delete `Fallible`; keep declared `impl Source => Target`; amends D-ERR2 and D-LIB3 |
| D-FAIL-CTX1 | A — note after `?`; automatic journey; delete `.context`; amends D-ERRCTX1 |
| D-FAIL-BREACH1 | A — registered E30xx report and one Prelude renderer on every tier |
| D-FAIL-TIER1 | A — contracts check on every tier, erase under proof, and blame the right site; amends D-PREPOST1 |
| D-FAIL-EXIT1 | A — fallible `fn run()`, exit 1 for reported errors, 70 for stops, 101 for Jet defects |
| D-FAIL-UNIT1 | A — `fn save(path) ? E`; no arrow and no unit; amends S80 and S34 |
| D-FAIL-BIND1 | A — ambient `err` inside a fallible `??` fallback |
| D-FAIL-EDGE1 | A — target-native delivery with one report; D-FAIL-BREACH1 owns report words |

All eleven decisions are ratified. Their accepted terms are settled law.

## Implementation slate

The implementation children carry the settled terms once:

- #1527 — D-FAIL-MODEL1, D-FAIL-CARRIER1
- #1528 — D-FAIL-ERROR1
- #1529 — D-FAIL-CONV1
- #1530 — D-FAIL-BREACH1
- #1531 — D-FAIL-TIER1
- #1532 — D-FAIL-CTX1
- #1533 — D-FAIL-EXIT1
- #1534 — D-FAIL-UNIT1
- #1535 — D-FAIL-BIND1
- #1536 — D-FAIL-EDGE1

## Implementation shape

**Phase A — internal re-founding, no surface change.** One report carrier
and one renderer in the Prelude; every existing message string preserved
exactly; the scheduler, stream, FFI, and `JetParaRuntimeFailure` paths route
through it, and the para deferred-failure aggregation keeps its behavior on
the one carrier; TIR nodes for contracts; the parity ledger gains the
`.err.out` dimension. All goldens byte-identical except the defect cases (JIT wording,
lost spans) which get their own cards.

**Phase B — land the ratified-but-unbuilt on the substrate, built once.**
Structured `Err` (S80 as amended), the `?` note and journey (D-ERRCTX1 as
amended), the D-VALIDATE3 rule vocabulary, `Validate.over`, the E2402 retext,
entry-point relaxation, and `#Todo` to a real stop.

**Phase C — implementation children, each a coherent greenfield
migration.** The conversion-rail deletion, the breach code family with web
parity, the exit law, the fallback binding. Each deletes its replaced form
in the same change (greenfield rule; no aliases, no fallbacks).
