# Python API superiority audit

This document preserves evidence. It is not the work queue. Tower cards and
ballots in the tracking section own every open finding, dependency, decision,
and exit criterion.

## Decision

Jet does not yet beat Python across the four API surfaces in the mined videos.
There are no unqualified wins today.

Jet has stronger ingredients: static types, explicit effects and ownership,
typed variadics, bounded encoding, strict data errors, and inspectable compiler
facts. However, an ingredient is not a win when the public operation is harder,
missing, inconsistent, or behaves differently across execution tiers.

The most serious loss is not cosmetic syntax. A Jet generator resumes before
the consumer asks for another value. After a consumer stops, AOT ignores the
closed channel and can run the rest of the producer, including side effects.
Default JIT stops only when it reaches another `yield`. This contradicts Jet's
ratified pull-stream meaning and execution-tier parity. Python suspends at
`yield` and exposes explicit close and cleanup behavior.

The second broad loss is API calling. Jet argument labels do not bind by name.
They cannot reorder arguments or skip an earlier default. Python supports
keyword-only, positional-only, named binding, `*args`, and `**kwargs`. Jet's
typed variadics are safer than Python's open tuples, but the fixed call rule
makes evolving and configuring ordinary APIs worse.

The correct target is not one global source-count ratio. Raw counts expose
ceremony, but tokens are not equal. Brackets, labels, and explicit blocks can
make structure clearer and give the compiler facts that Python leaves hidden.
For each workflow, Jet must report source cost and explain each increase. Extra
syntax passes only when it makes intent or structure clearer, reduces reasoning
burden, or enables a named guarantee, diagnostic, or expert control. Incidental
setup, temporary values, and control flow still fail.

“Better in every possible way” cannot be tested because some properties
conflict. For example, Python's arbitrary runtime mutation helps metaprogramming
but defeats static guarantees. Jet can enforce a useful claim: no workflow has
unexplained ceremony or worse reasoning burden without a compensating product
win. Every workflow must also have at least one evidence-backed Jet advantage.
A loss blocks the relevant release claim until it has an owner and proof. Only
a ratified product-scope decision can exclude the workflow. A rejected remedy
leaves the loss scored.

## Scorecard

| Surface | Jet advantage | Python advantage | Current result |
| --- | --- | --- | --- |
| Calls and variadics | Typed homogeneous or trait-bounded variadics; no `Any` bag | Named binding, skipped defaults, reordering, positional-only and keyword-only contracts, callable signature binding | Python wins |
| Callable transforms | Explicit compiler-owned markers; effects and ownership remain visible | Arbitrary decorators, signature preservation with `wraps`, runtime signature inspection | Python wins for API composition |
| Generators | Declared `Stream<T>` result; one-pass use is clear | Exact suspension, `close`, cleanup, delegation, `send`, and `throw`; no OS thread per generator | Python wins; Jet has a P0 correctness defect |
| JSON | Typed `#Codable`, `DataTree`, bounded streams, strict errors, canonical mode, atomic file work | Mature hooks, exact decimal parsing, formatting controls, direct file helpers | Jet's edition-2027 design can win; the incomplete edition split and docs prevent the claim |

## Competitive API gate

Every competing Core API workflow must appear in a maintained inventory, with
beginner, expert-policy, failure, and lifecycle cases where they apply. Every
inventoried workflow must pass all of these checks.

That inventory is [the Core surface ledger](../reference/core-surface-ledger.md).
It is generated from the compiler tables and from a recorded surface for each
competitor language. Read it; do not start a second inventory. This audit
predates the owner's 2026-08-03 ruling, which moved the bar from Python alone
to every language Jet competes with, and the ledger scores all eleven.

1. **Source and reasoning cost:** report lexical tokens, statements, calls,
   named temporary values, mandatory concepts, hidden facts, and nonlocal
   lookups. Do not use one fixed ratio. Each extra Jet construct must improve
   clarity, local reasoning, guarantees, diagnostics, or expert control.
2. **Safe default:** expected failure is typed; resource cleanup is automatic;
   unbounded work, ambiguous data, and hidden mutation do not enter silently.
3. **Expert control:** bounds, precision, scheduling, ownership, effects, and
   generated behavior are selectable without replacing the beginner API.
4. **Composition:** the API works in higher-order, generic, streaming, and
   framework code without erasing parameter modes, effects, errors, or types.
5. **Discoverability:** completion, hover, diagnostics, examples, and
   `jet inspect` expose the callable contract and derived behavior.
6. **Lifecycle:** cancellation, early exit, partial consumption, failure, and
   cleanup have one specified meaning.
7. **Tier parity:** interpreter, default JIT, AOT, and web when applicable have
   the same output, errors, side effects, cancellation, and cleanup.
8. **Evidence-backed advantage:** Jet wins at least one of runtime, memory,
   artifact delivery, safety, diagnosis, bounds, audit evidence, readability,
   or reasonability on the matched case. Measure machine properties. Use a
   structured record and independent review for readability and reasonability.

Raw source counts are evidence, not a verdict. If Python needs 12 tokens and
Jet needs 15, the three-token increase starts a review; it does not fail by
itself. Brackets still count, but they can pass when they expose structure that
improves reading and reasoning. Each extra construct is classified as
task-essential, clarity-bearing, guarantee-bearing, expert control, or
incidental ceremony. Incidental ceremony fails. For each source increase,
record the exact construct, raw cost, claimed clarity, reasoning, local-fact,
or guarantee benefit, rejected shorter form, lost value, and reviewer verdict.
Required setup, imports, error handling, and policy concepts count when the
user must write or understand them. Each competing fixture must be
independently accepted as idiomatic and minimal for the same task, inputs,
outcome, and normal language contract. Python does not need to imitate Jet-only
guarantees.

The existing [Core API ergonomic laws](../spec/stdlib-api-laws.md) cover naming,
fallibility, ownership, effects, allocation, diagnostics, examples, and one
mechanism. They do not enforce competitive source cost, signature evolution,
call-site labels, lifecycle parity, inspection, or a peer benchmark. Add this
gate to that review instead of creating a second API rubric.

## Findings

### 1. Labels make configurable APIs worse than Python

Ratified S61 says labels are optional documentation. Binding remains positional,
labels never reorder, and only trailing defaults can be omitted. This prevents
the main API benefit of named arguments: callers cannot select one late policy
without repeating every earlier default.

The encoding law itself demonstrates the desired call:

```jet
writer :: json.writer(^out, canonical: true)?
```

The shipped checker rejects it because the second positional parameter is
`EncodingLimits`, not `Bool`:

```text
Error [E0112]: `writer` wants `EncodingLimits` for argument 2, but this is Bool
```

Current examples therefore write the longer form:

```jet
json.writer(^out, encoding.EncodingLimits.safe(), true)
```

Python binds keywords by name and can make parameters keyword-only or
positional-only. Its function grammar also supports `*args` and `**kwargs`.
Jet already has the better open-arity rule: `name: ...T` and trait-bounded
heterogeneous variadics. It should keep that safety. The loss is named binding,
not the lack of a dynamic keyword dictionary.

The owner-gated decision is narrow: amend S61 so a supplied label binds to that
parameter and can skip defaults, while argument expressions still evaluate in
source order. Public APIs must be able to require labels where a positional
call is unclear, especially Boolean policy parameters. The ballot must also
decide and prove keyword-only and positional-only contracts, or explicitly
ratify why Jet rejects each without losing the matched API workflows. This
audit does not invent another call mechanism.

Evidence: [S61 and D-VARIADIC1](../spec/syntax-decisions.md),
[encoding constructor law](../spec/encoding-decisions.md),
`crates/jet-sema/src/Sema/CheckerCoreLib/fixed_sigs.rs`, and
`crates/jet-sema/src/Sema/CheckerCoreLib/core_call.rs`.

### 2. Jet lacks a complete typed replacement for decorators

Python decorators accept arbitrary expressions and run at function definition.
`functools.wraps` copies selected metadata and exposes the wrapped callable;
`inspect.signature` exposes parameter kinds, names, defaults, and binding.
This supports caching, registration, access policy, tracing, retries, and web
frameworks with one reusable surface.

Jet is right to reject an unrestricted runtime decorator mechanism. Its
compiler-owned markers and type derives are more visible. However, ordinary
higher-order functions are not a complete replacement:

- function values only describe plain read parameters;
- functions with `&` or `^` parameters cannot become function values;
- there is no parameter-pack or signature-preserving transform;
- user derives apply to types, not callable policy;
- `jet inspect expand` has inline, memory, and web lenses, but no derive or
  callable-signature lens.

Jet therefore cannot yet express every legitimate decorator use while
preserving types, parameter modes, defaults, effects, result errors, returned
views, identity, and inspectability.

Do not copy Python's runtime mutation. Prepare an owner decision around the
required capability: a reusable callable policy must preserve the full checked
signature and must expand into inspectable source facts. Compare the smallest
typed transform, explicit middleware or policy values, and first-party markers
against real framework, retry, cache, tracing, and registration fixtures.
Select one mechanism only after those fixtures prove it.

Separately, add derive and callable-signature projections to the existing
`jet inspect expand` command. This reuses the ratified inspection mechanism and
does not require new syntax.

Evidence: [S47 and D-METADERIVE1](../spec/syntax-decisions.md),
`tests/ui/function_value_access_cannot_erase.stderr`, and
`Source/CmdExpand.rs`.

### 3. Generator lifecycle violates the ratified meaning and I9

The syntax law says `yield` suspends until the next pull. AOT instead creates a
detached OS thread and sends through `sync_channel(0)`. Receiving a value frees
the producer immediately, before the consumer requests another value. AOT
ignores a closed-receiver send error and lets the producer finish. JIT checks
the closed sender at the next yield, so it can still run code between yields.

A focused probe produced:

```text
# default jet run
producer resumed
consumer got 1
consumer done

# AOT
producer resumed
consumer got 1
producer finished
consumer done
```

The consumer breaks after the first value. Both tiers run a producer side
effect too early; only AOT runs the final side effect. Existing generator tests
break early without a producer side effect, so they cannot detect the defect.

Python documents suspension with retained local state and explicit `close`,
`throw`, `send`, and `yield from`. Jet does not need to copy `send` and `throw`;
tasks and channels are the safer coroutine mechanism. Jet does need exact pull
suspension, prompt cancellation, deterministic cleanup, explicit close where
RAII is insufficient, and one meaning across every execution tier.

The implementation fix is not syntax-gated. It is required by the existing
Stream decision and I9. Add hostile tests for early break, dropped consumers,
producer failure, cleanup, code between yields, nested delegation, blocked
producer wakeup, and three-tier side-effect parity. Then measure creation time,
pull latency, peak memory, and cancellation for 1, 1,000, and 10,000 inactive
or partly consumed generators against Python. A thread per generator is not an
acceptable unmeasured claim.

Evidence: [generator law](../spec/syntax-decisions.md),
`crates/jet-codegen/src/Codegen/TIR/emit/functions.rs`,
`crates/jet-codegen/src/Codegen/TIR/emit/expressions.rs`,
`crates/jet-codegen/src/Codegen/TIR/lower/statements.rs`,
`crates/jet-jit/src/jit/lower_ctx.rs`, and `tests/dev.rs`.

### 4. JSON has the stronger design but not yet the stronger shipped API

Jet's intended API is better for production boundaries:

- `DataTree` is the one dynamic tree;
- `#Codable` provides typed encode and decode;
- missing and explicit null remain distinct;
- streaming constructors take safe limits;
- errors carry format, location, path, reason, and cause;
- canonical JSON is separate from pretty JSON;
- file output is moving to atomic replacement through card #288.

Python's default JSON decoder accepts non-finite numbers and keeps the last
duplicate object name. It provides no format-level input size or nesting limit.
Jet should beat those defaults, not imitate them.

The edition boundary still blocks a present-tense superiority claim:

- edition 2026 intentionally retains frozen prototype signatures and bytes,
  while edition 2027 owns the strict migrated surface;
- the Core reference does not consistently label which `DataTree`, `JSON`,
  `parse`, and `decode` descriptions apply to each edition;
- the ratified labeled writer example does not compile;
- strict canonical behavior is ratified for edition 2027, but edition 2026
  correctly remains the infallible prototype until explicit migration;
- `DataTree` numbers are `Int` or `Float`, so an incoming decimal token can lose
  precision before typed `Decode<Decimal>` sees it;
- Jet's `Decimal` decoder accepts `DataTree.Int` and `DataTree.Text`, but rejects
  fractional `DataTree.Float`; exact fractional values require quoted text.

Python's `parse_float=Decimal` can preserve an incoming JSON decimal token as an
exact decimal. Jet must offer an equally direct, typed path without adding an
untyped hook bag. The owner-gated requirement is: typed JSON decode into
`Decimal` and `BigInt` must preserve the original valid numeric value, or the
API must expose a checked number policy that does. Decide whether the canonical
tree gains exact number variants or typed decoding reads tokens before a lossy
tree conversion. Keep one codec model.

Formatting does not need Python's large option bag. Jet can win with three clear
operations: compact interoperable output, pretty human output, and canonical
hash/signature output. Each must be complete, bounded, and tier-identical.

Evidence: [encoding decisions](../spec/encoding-decisions.md),
[Core encoding reference](../reference/core-library.md),
`crates/jet-sema/src/Sema/CheckerCoreLib/fixed_sigs.rs`, and
`crates/jet-codegen/src/Prelude/CoreLib/Top/EncodingTraits.rs`.

## Ranked backlog

1. **P0 — Restore exact Stream pull, cancellation, cleanup, and tier parity.**
   This is an existing-law correctness fix, not a new feature. Add the hostile
   lifecycle matrix before changing the implementation. Tower #1392 owns this
   exact defect; completed #1216 proved a narrower parity fixture.
2. **P0 — Decide and implement API-grade argument labels.** Prepare an owner
   ballot amending S61. Tower #1393 and D-APILABEL1 own the choice and work.
   Prove skipped defaults, reordered labels, source-order evaluation,
   required-label or keyword-only contracts, positional-only contracts or a
   ratified workflow-complete rejection, and the exact
   `json.writer(^out, canonical: true)` call on all tiers.
3. **P0 — Ship the ratified JSON edition split and correct its documentation.**
   Keep edition 2026's frozen prototype unchanged. Make edition 2027 expose the
   already-decided strict `DataTree`/`#Codable` surface, migration rewrites,
   errors, and canonical behavior. Label both editions precisely in reference
   docs, sema signatures, examples, and tier tests. Tower #1394 owns this work.
   Completed #1213 only reconciled one example's JIT ledger.
4. **P1 — Add exact typed JSON numeric decoding.** After #1394 ships, resolve
   `Decimal` and `BigInt` without a dynamic callback bag or a hidden amendment
   to canonical JSON. Tower #1395 and D-JSON-EXACTNUM1 own the choice and work.
   Test exponent forms, large
   integers, scale, overflow, duplicate keys, non-finite values, limits,
   streams, and all tiers against Python's exact parse hooks.
5. **P1 — Prepare one signature-preserving callable policy choice.** Use real
   cache, retry, trace, registration, and route prototypes. Tower #1396 owns the
   research and must produce a decision-complete ballot after #1393. It must
   define the full signature type and reconcile the proposal with Jet's
   ratified metaprogram ceiling. Do not add Python-style arbitrary runtime
   decorators or ship syntax before owner ratification.
6. **P1 — Extend existing inspection.** Add derive expansion and full callable
   signature facts to `jet inspect expand`, including machine-readable output.
   Active #1388 owns general `inspect expand --json`. Tower #1397 is its blocked
   non-duplicate successor after both #1388 and #1396 for the two new lenses.
7. **P1 — Make Python API parity a release gate.** Tower #1442 owns the closed
   method-level Core-versus-Python ledger. Tower #1398 consumes that ledger and
   owns the release gate. Extend the Core API rubric and reuse the agent corpus
   machinery for independently minimized beginner and expert fixtures. Card
   #1171 owns only the agent-workload Python baseline; do not expand it
   implicitly. Report raw source cost, but reject only unexplained ceremony or
   a worse reasoning trade with no compensating product win.
8. **P2 — Complete syntax discovery.** Add variadics, spread, `Stream<T>`, and
   `yield` to `docs/reference/syntax-surface.jet`; prove hover, completion, and
   diagnostics teach the same canonical forms. Tower #1398 owns this exact exit
   criterion and incorporates the related surfaces from #1392, #1393, and the
   eventual #1396 ballot.
9. **P2 — Benchmark generator resources.** Compare inactive and partial streams
   with Python for memory, creation, pull, early close, and cleanup. Use the
   result to decide implementation, not marketing prose. Card #1392 owns this
   benchmark as an exit criterion.

## Tower tracking

| Finding | Work owner | Owner choice | State |
| --- | --- | --- | --- |
| Stream pull, cancellation, cleanup, and tier parity | #1392 | Existing law | Ready |
| Argument labels and parameter contracts | #1393 | D-APILABEL1=A ratified | Plan |
| JSON edition 2026 and 2027 split | #1394 | D-JSONCANON1 and encoding law | Ready |
| Exact Decimal and BigInt JSON numbers | #1395, after #1394 | D-JSON-EXACTNUM1=A ratified | Blocked |
| Signature-preserving callable policies | #1396, after #1393 | Ballot produced, then implemented by card | Blocked research |
| Derive and callable inspection lenses | #1397, after #1388 and #1396 | Existing inspect law | Blocked |
| Method-level Core-versus-Python ledger | #1442 | Existing API laws | Ready |
| Python API release gate | #1398, after #1392–#1397 and #1442 | Audit policy | Blocked |

## Existing ownership and duplicate check

- #1213 is done and covers one repaired JSON example's tier ledger, not the
  public JSON contract.
- #1215 is done and proves existing variadic and callable fixtures on JIT; it
  does not provide name-binding labels or signature transforms.
- #1216 is done and proves the existing generator example across tiers; its
  fixture does not observe producer side effects after early exit.
- #1388 is active for machine-readable `jet inspect expand` output.
- #288 is active for atomic file replacement and remains part of the JSON win.
- #769 is done for the corpus framework. #1171 owns only the agent-workload
  Python baseline and is currently blocked by #1170; it does not own this Core
  API inventory.

Cards #1392–#1398 and #1442 are the durable work owners. D-APILABEL1=A and
D-JSON-EXACTNUM1=A are ratified. Card #1396 must prepare the callable-policy
ballot and then implement its ratified result. The audit remains evidence and
does not act as a second backlog.

## Method and sources

This is a day-zero artifact comparison focused on the four mined API clusters.
It amends, rather than repeats, the broad
[Python conversion field audit](field-audit-python-2026-07-26.md) and the
[Python brevity research](../research/surface-research-python-brevity-2026-07-26.md).
It compares shipped Jet source, tests, examples, and CLI behavior with current
official Python 3.14 documentation:

- [Function definitions](https://docs.python.org/3.14/reference/compound_stmts.html#function-definitions)
- [Yield expressions and generator methods](https://docs.python.org/3.14/reference/expressions.html#yield-expressions)
- [`functools`](https://docs.python.org/3.14/library/functools.html)
- [`inspect.Signature`](https://docs.python.org/3.14/library/inspect.html#introspecting-callables-with-the-signature-object)
- [`json`](https://docs.python.org/3.14/library/json.html)
- [RFC 8259](https://www.rfc-editor.org/rfc/rfc8259.html)

The label mismatch was reproduced with `target/debug/jet check`. The stream
lifecycle mismatch was reproduced with the existing debug Jet binary under
default run and AOT. A fresh workspace build was attempted first, but unrelated
current edits in `crates/jetpack/src/Overlay.rs` do not compile. Source lowering
independently confirms both stream behaviors. No broad performance or memory
claim is made without a benchmark.
