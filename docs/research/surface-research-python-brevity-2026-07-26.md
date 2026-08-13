# Beating Python on source size

Date: 2026-07-26  
Revision: owner feedback on compact braces, batteries, comprehensions, and examples

Vocabulary: [Jet vocabulary](../spec/vocabulary.md).

## Decision

Jet should make source brevity a product requirement.

The target is not merely to approach Python. Jet should:

- beat Python on physical lines and words across a broad, matched task corpus;
- beat Python by a wide margin where Jet can derive checked behavior;
- keep Rust-grade control available through the same mechanism;
- keep safety, failure policy, bounds, ownership, and audit facts honest;
- make the shortest safe Jet form the form that examples and tools teach.

The current four-program audit does not meet that target. The committed Jet
adapters use 1.76 times Python's physical lines and 1.98 times its words. They
use a low-level subset of Jet and do not prove the best current Jet surface.

Four corrections change the strategy.

First, braces are not a fixed line tax. Ratified formatter decision D-FMT1
already preserves an author-written, one-simple-statement body on one line.
That rule covers functions, methods, loops, and simple conditional bodies.
Some of the 41 brace-only lines in the measured adapters are therefore an
example and authoring gap. The adapters must be rewritten before Jet can say
how many are avoidable. The full count is not an unavoidable language cost,
but multi-statement and nested bodies will retain their braces.

Second, built-ins must be exceptional. The beginner face should feel like a
safer, typed, batteries-included Python. The expert face should offer
Rust-grade policy, ownership, allocation, scheduling, and audit control.
These faces must expose one mechanism at different depths.

Third, comprehensions are open for an owner decision. Tower card #1204 now
carries D-COMPREHENSION1. Its recommendation is a source-first Jet form:

```jet
names :: [loop user; users if user.active => user.name]
```

It matches Python's one-line result while stating the binding, filter, and
projection in execution order. No comprehension syntax is ratified until the
owner resolves that ballot.

Fourth, canonical examples must teach current Jet. This revision updates four
examples to show one-line simple bodies, immutable bindings, compound
assignment, and expression lambdas. The changed examples fall from 148 to 130
physical lines. Their whitespace-word count falls only from 455 to 447. That
result proves why Jet needs both a layout strategy and a semantic compression
strategy.

## Current measured gap

The four matched programs contain:

| Measure | Python | Jet | Gap |
| --- | ---: | ---: | ---: |
| Physical lines | 125 | 220 | +95 |
| Nonblank lines | 109 | 214 | +105 |
| Words | 321 | 635 | +314 |
| Import or `use` lines | 10 | 11 | +1 |
| Brace-only lines | 0 | 41 | +41 |
| Loop headers | 5 | 12 | +7 |
| Conditional headers | 19 | 33 | +14 |
| Explicit `raise`, `try`, or `except` lines | 10 | — | — |
| Explicit `panic` calls | — | 23 | — |
| `??` fallback sites | — | 13 | — |

Words here mean nonempty Unicode-whitespace runs. They are not lexical tokens.

Imports are not the problem. Memory capability marks are not the main problem.
The Jet adapters contain six copies and no edits or takes.

The gap has two distinct parts.

### Physical-line gap

Forty-one Jet lines contain only a brace. D-FMT1 allows many simple bodies to
stay on one line:

```jet
if ready { launch() }
loop item; items { print(item) }
fn one() => Int { return 1 }
fn area(self) => Float { return (self.width * self.height) }
```

The formatter preserves this author choice when the body has one simple
statement, no inner comment, and fits the 100-column limit. A multiline body
stays multiline. Nested or mixed control flow expands.

Jet should use this form when the whole operation is easier to scan as one
unit. It should not place several statements on one line.

### Word and logic gap

Moving braces does little to word count. The larger gap comes from:

- seven extra loops;
- fourteen extra conditional headers;
- manual accumulators and temporary collections;
- repeated panic fallback text;
- missing use of current collection and text operations;
- no compact comprehension form today;
- Python's broad and familiar built-ins;
- compact Jet forms whose execution-tier parity is not yet proved by the
  benchmark.

Jet beats the physical-line gap through readable inline bodies and removal of
whole blocks. It beats the word gap through better operations, inference,
derived batteries, and one compact collection construction form if ratified.

## Why the gap exists today

### 1. The measured Jet adapters use a low-level subset

This is the largest avoidable cause.

Across the four Jet adapters, the relevant compact terminals appear almost
nowhere. The code does not use:

- `#CLI`;
- `map`, `filter`, or `filter_map`;
- `group_by` or `count_by`;
- `indexed`;
- `String.lines`;
- `try_collect`;
- list spread;
- text patterns;
- fallible `run`.

The effect is visible in every task.

`repository_marker_scan.jet` uses a six-line loop to count matching lines. It
then uses another loop to print rows. Lazy adapters, reducers, and `join`
already cover much of this work.

`git_diff_review.jet` uses fifteen lines to split the first path segment. Jet
already has `String.before`, `String.after`, and text patterns. The adapter
also uses a branch tree and three counters where keyed counting may express
the same rule.

`incident_report.jet` keeps a separate service list and scans all incidents for
every service. `group_by`, `count_by`, `indexed`, destructuring, and text
patterns can remove much of that control flow.

`process_batch.jet` builds an argument list through a push loop. Jet already
has list spread:

```jet
[~program, ...arguments.split("|").to_list()]
```

The first benchmark action remains an idiomatic rewrite with frozen behavior.
It must run the same success and hostile cases.

### 2. Built-in breadth is not yet treated as a competitive surface

Python succeeds because common operations are easy to find and compose. Its
built-ins and standard library cover iteration, sorting, aggregation, text,
files, processes, data formats, dates, networking, testing, and inspection.

Jet has strong pieces, but the benchmark shows that users can still fall into
manual control flow. A built-in is not successful merely because it exists.
It must be:

- named predictably;
- available without a package hunt;
- discoverable from the value or task;
- concise in the common case;
- precise about errors and bounds;
- consistent across similar types;
- available in default run, AOT, comptime, and development execution where
  its type permits;
- backed by an expert path for policy and inspection;
- used by canonical examples.

Python-like Jet should be the beginner battery surface. Rust-like Jet should
be the expert control surface. They should not be separate libraries or
competing idioms.

### 3. Compact checked forms need fresh execution-tier proof

A short surface is not usable if authors must replace it with loops to keep
`jet run` working.

The maintained ledgers still contain entries for iterator audit, text-pattern,
and list-spread examples. See `tests/jit_gaps.txt` and
`tests/jit_corpus_gate.txt`. Fresh runs on 2026-07-26 show that all four
examples changed in this revision now pass normal `jet run`, including
`iter_tools_audit.jet`.

The iterator-audit entry is now proved stale. The text-pattern and list-spread
entries remain recorded failures until their examples receive separate fresh
runs. Card #688 owns this execution-parity class and the ledger refresh.

Every compact idiom used for a source-size win must pass:

- `jet check`;
- default `jet run`;
- AOT run;
- its hostile inputs;
- formatter round-trip.

Execution parity is part of the brevity strategy.

### 4. Python hides failure and type work

Python exceptions propagate without a return annotation or a call-site mark.
A handler can be far from the failure. The
[Python compound statement reference](https://docs.python.org/3.14/reference/compound_stmts.html)
defines that stack search.

Jet exposes fallibility through `T ? E`, `?`, and `??`. That adds useful
information, but the measured adapters often pay more than Jet requires. They
contain thirteen `??` fallbacks, usually with repeated panic text.

A fallible entry can keep the checked channel and name the failure once:

```jet
fn run(args: Args) => Void ? {
    text :: fs.read(args.input).context("cannot read input")?
}
```

Jet should not copy implicit exceptions. It should make checked propagation
the shorter normal path and make added context easy when it matters.

### 5. Python has a compact collection construction form

Python can combine an output expression, a loop, and filters inside one list,
set, dictionary, or generator expression. The
[Python expression reference](https://docs.python.org/3.14/reference/expressions.html)
defines its nested evaluation rules.

Jet currently uses pipelines:

```jet
names :: users.filter(user => user.active).map(user => user.name).to_list()
```

That form is composable and explicit about laziness. It is also longer and
repeats lambda structure.

D-COMPREHENSION1 reopens the old decline with a Jet-cohesive design. The
recommended option is:

```jet
names :: [loop user; users if user.active => user.name]
```

The proposed order is the important difference:

1. bind `user` from `users`;
2. test `user.active`;
3. produce `user.name`.

The proposal reuses brackets, `loop`, `if`, and the lambda arrow. It owns
a local scope and lowers to existing collection and iterator semantics.
Ordinary loops and pipelines remain the expanded forms.

The ballot also covers ordered nested loops, map projection, ownership,
fallibility, effects, formatter behavior, and execution-tier parity. The
owner may choose source-first, result-first, or no comprehension syntax.

### 6. Examples have taught avoidable ceremony

Examples are executable specifications and training data for agents.

This revision changes:

- `examples/features/basics/named_args.jet`;
- `examples/features/text/string_parse.jet`;
- `examples/features/collections/iter_tools_audit.jet`;
- `examples/features/concurrency/task_group.jet`.

The examples now show:

- simple functions and methods on one line;
- one-statement loops on one line;
- `::` for values that do not mutate;
- `+=` instead of repeated assignment;
- expression lambdas for one-expression tasks.

Measured change:

| Example set | Before | After | Change |
| --- | ---: | ---: | ---: |
| Physical lines | 148 | 130 | -18 (-12.2%) |
| Words | 455 | 447 | -8 (-1.8%) |

The line reduction is real and readable. The smaller word reduction confirms
that built-ins and higher-level forms must do the rest.

## The dual-facet product rule

Jet should beat Python and Rust through progressive disclosure.

The beginner path should provide:

- safe defaults;
- batteries included;
- inference where the answer is unambiguous;
- one-call or one-expression common tasks;
- helpful errors in Jet terms;
- no required ownership or policy vocabulary until behavior needs it.

The expert path should provide:

- explicit ownership and memory movement;
- allocation and buffering policy;
- concurrency scheduling and cancellation policy;
- resource and output limits;
- target and backend control;
- generated-code and lowering inspection;
- exact error and audit types;
- predictable performance.

These paths must share one semantic mechanism.

For example, a beginner collection operation may infer the result type and
allocation. The expert form may select capacity or a fallible collector. Both
must use the same iteration, ownership, effect, and error laws.

A beginner process helper may choose safe capture limits. The expert builder
may select streaming, environment, timeouts, and cancellation. Both must use
the same process model and diagnostics.

This is how Jet can offer Python's ease without Python's hidden global state,
unchecked shapes, implicit exception flow, runtime packaging burden, or
environment ambiguity.

## The exceptional built-in standard

Jet should treat built-ins and Core APIs as a competitive product.

### Admission test

A new operation should enter the default battery set only when:

1. At least three real corpus tasks need the same operation.
2. It completes an existing type or mechanism.
3. Its name is predictable from related operations.
4. It removes manual control flow or policy-free glue.
5. It does not hide a meaningful safety decision.
6. It has a clear expert extension path when policy matters.
7. It works across required execution tiers.
8. It has executable examples and precise diagnostics.
9. It beats or matches the strongest peer form on the matched task.

This rule prevents benchmark-specific helpers while still demanding broad
batteries.

### Coverage priorities

The first competitive sweep should cover:

| Domain | Beginner battery goal | Expert control goal |
| --- | --- | --- |
| Collections | direct transforms, grouping, sorting, aggregation, construction | laziness, allocation, duplicate policy, fallible collection |
| Text | lines, fields, search, replace, parse, patterns | Unicode mode, bounds, streaming, allocation |
| Paths and files | common reads, writes, walks, metadata | atomicity, links, permissions, buffering, limits |
| CLI | typed arguments, help, completion, errors | exact grammar, environment, audit projection |
| Data | derived encode and decode | schema, limits, compatibility, canonical form |
| Processes | safe run and capture | streams, environment, limits, timeouts, cancellation |
| HTTP and services | typed request and response paths | body limits, pools, retries, timeouts, observability |
| Concurrency | structured tasks, race, all, channels | scheduling, capacity, cancellation, audit |
| Time | parse, format, duration, deadlines | clock choice, timezone, ambiguity policy |
| Testing | assertions, cases, properties, fixtures | seeds, budgets, isolation, replay evidence |

The table defines product goals, not approved API spellings.

### Competitive review

For each domain, compare real tasks against:

- Python for discovery, terseness, and batteries;
- Rust for control, types, performance visibility, and ecosystem quality;
- another domain leader when useful.

The comparison must record:

- lines, words, lexical tokens, and files;
- accepted and rejected inputs;
- hidden defaults;
- required imports and dependencies;
- configuration and deployment source;
- failure and cleanup behavior;
- discoverability from editor completion and `jet help`;
- whether the compact form works in every promised tier.

Jet should copy strong outcomes, not peer syntax by reflex.

## Where Jet can beat Python

Jet will not win by making `print("hello")` shorter. It can win when the
compiler replaces work that Python authors must write, configure, or test.

### Typed command lines

```jet
#CLI
struct Args {
    input: Path
}

fn run(args: Args) => Void ? { process(args.input)? }
```

The fair Python comparison includes parsing, conversion, help, completion,
error behavior, and the command contract. A raw `sys.argv` lookup is not the
same task.

### Typed decoding

`#Codable` can replace generic dictionary access, field conversion, missing
field checks, and repeated serialization glue. The matched Python program must
enforce the same wire shape and failure policy.

### Checked parsing

Jet text patterns can bind and convert in one operation:

```jet
if line == {
    "{service}\t{status}\t{duration:Int}" -> accept(service, status, duration)
    else -> reject()
}
```

Python commonly splits, checks arity, unpacks, and converts. Jet can be shorter
while producing a more precise error.

### Collection construction

Current adapters can remove loops through `map`, `filter_map`, `group_by`,
`count_by`, `partition`, reducers, spread, and `try_collect`.

If D-COMPREHENSION1 selects source-first syntax, the simplest eager
filter-and-project task also gains a direct one-line form. Pipelines remain the
lazy and expert composition path.

### Failure-aware pipelines

```jet
values :: rows.map(row => parse(row)).try_collect()?
```

This can be shorter than a Python loop with a `try` block while preserving a
typed failure. Jet should also reject unused lazy chains and repeated
consumption when the type cannot support it.

### Structured concurrency

Python must choose threads, processes, or `asyncio`. A complete `asyncio` flow
often adds async declarations, awaits, task creation, and lifecycle handling.

Jet's `taskgroup` keeps synchronous-looking code and owns cleanup. Matched
cancellation, race, timeout, and error cases should count the whole lifecycle.

### Deployment and environment

The source-only chart must remain visible. A second chart should count the
whole human-maintained solution:

- application source;
- dependency and lock declarations;
- type-checker configuration;
- validation models;
- tests needed for the same guarantees;
- package and deployment files.

Jet can win this chart because one compiler and one native artifact can replace
separate runtime, checker, bundler, and environment work. The whole-task win
must never hide a verbose language surface.

## Strategy

### Stage 1 — Correct layout and examples now

Use ratified one-line bodies when they improve scanning:

- one simple statement only;
- no inner comment;
- no nested block;
- within the formatter width;
- author intent preserved.

Do this across first-hour and flagship examples. Keep multiline bodies where
the code has several steps or deserves visual weight.

This stage improves physical lines. It is not the word-count strategy.

### Stage 2 — Rewrite the frozen benchmark idiomatically

Use #769 and #1171 as the evidence stream.

For every pair:

1. Pin language versions, formatters, line width, and configuration.
2. Review Python for idiomatic Python.
3. Review Jet for the shortest current safe Jet.
4. Preserve the same successful and hostile outcomes.
5. Run default and AOT Jet paths.
6. Record physical lines, normalized lines, words, lexical tokens, and files.
7. Name the guarantee bought by every retained Jet premium.

Rewrite in this order:

1. Use `#CLI` where the task has a command contract.
2. Replace repeated `?? panic` with contextual `?` where behavior permits.
3. Replace split and nested lookup with exact destructuring or text patterns.
4. Replace manual indexes with `indexed`.
5. Replace push loops with transforms, spread, or a ratified comprehension.
6. Replace nested scans with grouping and counting.
7. Replace print loops with `join` where output rules permit.
8. Remove temporary collections when a terminal can consume the iterator.

A shorter program that changes empty-field handling, order, cleanup, or errors
is a loss.

### Stage 3 — Close compact-form execution gaps

Every brevity feature must work through normal `jet run`, AOT, and any other
promised tier. Route execution failures to #688 or the exact runtime owner.

Compact errors must point to the user's operation. They must never expose
generated Rust or a backend boundary.

### Stage 4 — Resolve and implement the comprehension decision

D-COMPREHENSION1 on #1204 presents three choices:

- source-first comprehension;
- result-first comprehension;
- pipelines and loops only.

The recommendation is source-first because it matches Python's one-line
outcome while improving reading order.

If ratified, implementation must be a complete vertical slice:

- syntax registry and amended decision record;
- parser and formatter;
- sema and diagnostics;
- ownership, effects, and failure behavior;
- list and map construction laws;
- iterator or loop lowering;
- JIT, AOT, comptime, and development parity;
- examples, goldens, snapshots, and hostile tests.

No code should use the proposed syntax before ratification.

### Stage 5 — Build the batteries gap ledger

For every corpus loss, classify the cause:

- missing current idiom;
- missing execution parity;
- missing generic operation;
- missing inference or derivation;
- required safety or policy source;
- task-specific application logic.

Only the third and fourth classes can propose new built-ins. Apply the
three-task admission rule and compare the full operation against peer
libraries.

Each accepted operation needs:

- the smallest safe beginner form;
- the expert policy form when needed;
- one semantic implementation;
- editor and help discoverability;
- examples that lead with the common form;
- benchmark proof.

### Stage 6 — Make compact Jet the taught Jet

Update first-hour, CLI, file, process, data, collection, and concurrency
examples.

Each common task should show:

1. the shortest safe form;
2. inferred or generated facts through inspection;
3. the lower-level expert form when it teaches real control.

After canonical rewrites exist, `jet fix` may suggest them. Useful candidates
include push-loop to transform, nested counts to `count_by`, manual indexes to
`indexed`, list construction to spread, and repeated panic fallback to `?`.

The tool must suggest an existing mechanism. It must not invent a second style.

### Stage 7 — Expand the corpus and hold the gate

Cover at least:

- CLI;
- files and paths;
- text parsing;
- processes;
- HTTP;
- SQLite;
- typed data;
- tests;
- notebooks;
- browser work;
- structured concurrency.

Use two scoreboards:

1. **Outcome parity:** the same observable task result.
2. **Assurance parity:** the same validation, bounds, failure policy, cleanup,
   and deployment contract.

Jet should win both. It should win the assurance board by more.

## Binding measurement targets

Freeze these rules in the corpus:

- Each task has equal weight.
- The aggregate ratio is total Jet units divided by total Python units.
- Median and 90th percentile use per-task ratios and nearest rank.
- Physical lines use pinned formatter output.
- Normalized lines remove blank and comment-only lines but retain braces.
- Words use nonempty Unicode-whitespace runs.
- Lexical tokens use each pinned language lexer as a separate measure.
- Author-written one-simple-statement lines are valid source, not minification.
- Several statements packed onto one line are invalid benchmark source.

The broad claim needs all of these gates:

- At least twelve idiomatically reviewed Python and Jet pairs.
- Aggregate and median Jet physical-line ratios below 1.0.
- Aggregate and median Jet word ratios below 1.0.
- The 90th percentile for either ratio at or below 1.25.
- Jet wins at least 70% of assurance-parity tasks.
- Jet reaches at most 0.8 times Python's lines and words in at least three
  strength domains.
- Every compact Jet solution passes default run and AOT.
- No win depends on several statements per line or a task-specific Core helper.
- Generated behavior is inspectable and covered by outcome tests.
- Every retained source premium names its safety or operational result.
- Repair and refactor trials show that compact code remains easy to change.

Publish aggregate totals, medians, percentiles, and every task ratio. Averages
alone can hide a severe loss.

## Work ownership

- #769 and #1171 own corpus, scoring, and Python baseline work.
- #1204 and D-COMPREHENSION1 own the comprehension choice.
- #743 is the completed iterator foundation.
- #688 owns default and AOT execution parity.
- #745 is the completed zero-copy parser audit.
- #288 owns `core.files` and `core.path`.
- #301 owns the existing HTTP surface.
- #237 owns typed data closeout.
- #180 and #1125 own the Python bridge and cross-language conformance.

Keep measured losses in #769 and #1171 until they identify one repeated
mechanism. Route tier failures to #688. Do not create a general “make Jet
concise” umbrella.

## What not to do

Do not:

- clone Python comprehension order without comparing reasoning cost;
- add a comprehension that creates a second iterator or ownership model;
- add top-level script statements beside `fn run`;
- add aliases for `::`, `:=`, `&`, `^`, or `~`;
- hide fallibility to match implicit exceptions;
- remove useful bounds or typed errors;
- add one Core helper per benchmark;
- pack several statements onto one line;
- force every simple author-written body into multiline layout;
- compare checked Jet with unchecked Python and call the proof “verbosity”;
- claim a generated-code win when users cannot inspect the behavior;
- call a compact form shipped when normal `jet run` rejects it.

## Recommended product position

The current claim should remain honest:

> Jet is not yet shorter than Python in the measured corpus. The gap is now
> understood: low-level examples, underused batteries, execution-tier proof
> debt, checked failure source, and no compact collection construction form.

The intended product promise is stronger:

> Jet gives beginners Python-like batteries with safer defaults. The same
> operations reveal Rust-like control when experts need it. Common code should
> be as short as Python, easier to reason about, and more explicit only where
> the extra source buys a checked result.

The path is measurable. Fix readable layout now. Rewrite the corpus with
current Jet. Resolve source-first comprehensions. Build exceptional batteries
from repeated gaps. Require every compact form to work in every promised tier.
Then hold Jet below Python on both physical lines and words without weakening
the outcome.
