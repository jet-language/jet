# Python conversion field audit

Date: 2026-07-26

## Decision

Jet is not easier than Python overall today.

For the four matched automation programs already committed in Jet's agent
workload corpus, Jet uses 1.76 times as many source lines and 1.98 times as many
words. Individual programs use 1.56–2.07 times the lines and 1.88–2.17 times the
words. This is a real conversion tax. It is not mainly caused by Jet's memory
capability sigils. The four Jet programs contain six explicit copies, no edits,
and no takes. Most of the extra source is typed setup, explicit result handling,
collection loops, and less compact text and data manipulation.

Jet's present advantage is therefore not fewer keystrokes. It is earlier and
more local proof: static types, explicit errors, visible mutation and ownership,
compiler-inferred effects with optional explicit bounds and tool-visible
provenance, structured concurrency, and a native artifact. Python usually wins
the first draft. Jet's value hypothesis is that it lowers the cost of later
changes, incidents, and concurrent rewrites. The current corpus does not measure
that hypothesis. Jet already has a concrete deployment advantage when a target
machine should not carry a language environment.

That is a credible focused pitch for command-line tools, automation, services,
and data pipelines where correctness and native delivery matter. It is not yet
a credible general replacement pitch for Python. Installation, AOT edit-run
latency, Python migration, notebook interaction, compact data work, and several
first-party examples must improve before Jet can ask most Python users to
convert.

The recommended product target is clear:

- Keep Jet within 1.25 times Python's source tokens for ordinary, safe tasks.
- Permit up to 1.5 times only when the extra source exposes a concrete safety or
  operational guarantee.
- Make the default warm script loop competitive with Python.
- Let users keep Python libraries behind a typed, explicit bridge while they
  migrate one boundary at a time.

## Scope and method

This is a day-zero field comparison. It scores the work a new user must do now.
It does not count project age, package count, community size, popularity, trust,
or familiarity as product wins or losses.

The audit used:

- four committed, executable Python/Jet adapter pairs under
  `tests/agent_workloads/adapters/`;
- eight exact-outcome workload cases, including malformed input, partial input,
  large standard error, and timeout recovery;
- representative shipped Jet examples for CLI parsing, files, processes, JSON,
  HTTP, data, tasks, and tests;
- the current compiler, default run cache, installer, notebook server, Python
  importer, and package commands;
- current official Python, PyPA, pip, PyPI, pytest, FastAPI, Requests, pandas,
  and Python concurrency documentation.

The matched corpus is evidence, not a proof of the shortest possible program in
either language. Before turning its ratios into a release gate, both sides
should receive an idiomaticity review by experienced users of that language.

## Measured source cost

| Task | Python lines | Jet lines | Line ratio | Python words | Jet words | Word ratio |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| Repository marker scan | 17 | 31 | 1.82 | 42 | 91 | 2.17 |
| Git diff review | 51 | 85 | 1.67 | 122 | 236 | 1.93 |
| Incident report | 30 | 62 | 2.07 | 95 | 179 | 1.88 |
| Process batch | 27 | 42 | 1.56 | 62 | 129 | 2.08 |
| **Total** | **125** | **220** | **1.76** | **321** | **635** | **1.98** |

The byte ratio is 1.79. All eight declared workload outcomes passed for both
languages on this audit run.

This sample says three useful things.

First, Jet's overhead is consistent. It is not one pathological task. Second,
the cost is close to double in words even when the line ratio is lower. Jet
asks the author to name more intermediate state and outcomes. Third, the
overhead buys real information, but the surface does not yet compress common
safe intent enough.

The next corpus revision should count lexical tokens and semantic operations,
not only lines and whitespace-separated words. It should also add HTTP client,
web route, SQLite, CSV aggregation, typed CLI, test fixture, notebook analysis,
and browser automation pairs. Paired change, repair, diagnostic, and refactor
cases must test Jet's maintainability hypothesis.

## Runtime and edit-run cost

Jet currently has two very different stories.

The focused warm-cache test measured the debug Jet no-op median at 4.874 ms,
versus 11.284 ms for Python, 16.019 ms for Node, and 2.129 ms for Bash. The Jet
release binary was absent, so the ratified two-times-fastest-peer gate was
skipped. This is encouraging evidence for the cached default JIT path, but it
is not release-gate evidence.

The matched workload runner uses `jet run --release`. Its fast Python cases took
about 20–26 ms. Jet took about 14–20 seconds, including AOT compilation. That is
roughly 550–970 times the wall time for these edit-run cases. The measurement
does not describe an already-built Jet executable. It does describe a workflow
that users will encounter unless the default mode, caching, and documentation
keep them away from repeated AOT builds.

This split must be explicit in product copy:

- default `jet run`: fast cached development loop;
- `jet run --release`: optimized artifact build, with cache reuse;
- built binary: startup and execution without compiler cost.

Cards #688 and #666 already own the parity and compiler-speed work. They should
close against user-visible edit-run scenarios, not compiler-only benchmarks.

## The memory capability and sigil question

Python makes assignment easy because a name normally refers to an object.
That ease hides decisions. Two names can alias the same mutable object; a
mutable default can be shared across calls; a closure can observe a later loop
value; and `+=` may mutate or replace depending on the type. Python documents
these behaviors in its
[programming FAQ](https://docs.python.org/3/faq/programming.html) and
[class tutorial](https://docs.python.org/3/tutorial/classes.html).

Jet makes more of the decision visible:

| Surface | Meaning for a Python user |
| --- | --- |
| `name :: value` | Bind a value that will not be reassigned. |
| `name := value` | Bind local state that may be reassigned. |
| bare argument | Let the callee read it. |
| `&value` | Let the callee edit it. |
| `^value` | Hand it to the callee. |
| `~value` | Keep an explicit copy. |
| `T ? E` | The operation returns a value or a named error. |
| `?` | Return the error to the caller. |
| `??` | Provide a fallback. |

In the 220 Jet lines measured here, `::` appears 28 times, `:=` 18 times,
`??` 13 times, and `~` six times. `&`, `^`, and propagation `?` do not appear.
Memory capabilities are therefore not the main day-to-day verbosity source in
this corpus. Binding syntax and explicit fallback handling are more visible.

The beginner lesson should not start with the term "memory capability." It
should teach three practical rules:

1. `::` does not change; `:=` may change.
2. Calls read by default; `&` edits and `^` hands over.
3. `~` means "I need my own copy."

The compiler should then explain the first exceptional case in Python terms:
"Python assignment would alias this value. Jet needs you to choose read, edit,
take, or copy." Beginner examples should avoid ownership marks until the task
needs them. Expert documentation should retain the full model and its audit
value.

The product bargain is sound. Python is easier when the object graph is still
in one person's head. Jet can be easier when code is reviewed, changed, made
concurrent, or maintained by someone else. Jet must prove that benefit without
making every local value look like a systems-programming exercise.

## Task-by-task usability

### One file and first hour

Python wins the first keystrokes: top-level statements and `print()` are enough.
Jet asks for `fn run()`. Jet then offers a typed program and native build without
changing languages.

Jet's current installation loses badly. The documented route requires cloning
the repository, using Nix, building, and adding `result/bin` to `PATH`. Python
installation varies by platform, and Python's packaging guide must discuss
virtual environments and externally managed installations, but a prospective
Jet user should not need to understand Jet's build environment. Card #399 is a
conversion gate, not a polish task.

### Files, text, and repository automation

Python wins source density through `pathlib`, iterators, comprehensions,
unpacking, slicing, and broad text helpers. The repository marker scan is 17
Python lines versus 31 Jet lines. Jet wins explicit result types, typed I/O
failures, deterministic sorted traversal, and fewer implicit conversions.

Jet needs compact safe iteration, parsing, sorting, grouping, and path idioms.
The answer should be library and inference improvements where possible, not new
syntax. A Python-to-Jet cookbook should show the direct replacement for common
`pathlib`, comprehension, `split`, `sorted`, and dictionary patterns.

### Command-line programs

Jet's `#Cli` derive is a strong conversion surface. Typed arguments, generated
help, validation, and native distribution can beat Python's standard
`argparse` experience. Python libraries such as Typer can match the declarative
style, but they add an environment and dependency.

Jet should lead its Python pitch with a complete CLI, not hello world. The demo
should include arguments, files, a subprocess, a useful diagnostic, tests, and
a single deployable artifact.

### Processes and shell automation

Python is shorter and has deep process controls. Python and Jet both accept
argument vectors without invoking a shell and both offer timeouts. Jet provides
typed builder controls and outcomes, including an optional output cap; these
controls are not all defaults. The process batch pair is 27 Python lines versus
42 Jet lines. A superiority claim needs matched default and failure-behavior
tests.

The workload corpus must expand through pipes, streaming, cancellation, process
groups, signals, terminal interaction, and large-output backpressure. Cards
#769 and #1171 are the correct evidence stream. Browser automation card #772
and any process-control successors should use the same Python baseline.

### Errors and missing values

Python exceptions make the happy path compact. Exceptions propagate implicitly
until caught, and annotations do not describe that effect. Python's
[errors tutorial](https://docs.python.org/3/tutorial/errors.html) documents
that runtime model.

Jet makes failure part of the type and call surface. This is more source in
small scripts and a major review advantage in services and automation. Jet
should improve inference and provide narrow `try`-style helpers without hiding
failure. It should not copy Python's implicit exception channel.

### Types

Python type annotations are useful but are not enforced by the runtime; Python
expects a separate checker to perform most static checking, as its
[typing documentation](https://docs.python.org/3/library/typing.html) states.
This lets Python teams choose gradual adoption, but it also creates different
truths across runtime, editor, checker, and configuration.

Jet avoids that split. The cost is that conversion cannot be purely mechanical
when Python relies on dynamic shapes. Jet should preserve gradual migration at
the boundary: imported Python remains explicitly foreign and checked Jet types
begin at the interface.

### JSON and serialization

Python wins quick inspection: decode a dictionary and keep moving. Jet can win
production maintenance through `#Codable`, typed fields, bounded input, and
named decode failures.

The shipped `examples/features/serde/json.jet` used an obsolete mixture of
`Json` and `DataTree` constructors. The example-quality pass repaired the
source contract, and the example now passes `jet check`. The default JIT still
reports an E0956 support gap on this path, so complete executable parity remains
compiler work. See `docs/audits/example-quality-2026-07-26.md`.

### HTTP clients

Requests makes the first request exceptionally small:
`requests.get(...)`, then `.json()`. Status failure remains a separate
`raise_for_status()` choice, as the
[Requests quickstart](https://requests.readthedocs.io/en/latest/user/quickstart/)
shows.

Jet is more explicit about effects, body limits, status, and decoding. That is
good for production and verbose for exploration. Provide one compact safe
default that fetches, checks success, bounds the body, and decodes a type. Keep
the lower-level response controls available.

### Web services

FastAPI can define a first route in a few lines, start a development server,
and expose generated OpenAPI and interactive documentation, as its
[first-steps guide](https://fastapi.tiangolo.com/tutorial/first-steps/) shows.
Jet has Core HTTP and full-stack examples, but its first-route surface remains
more manual.

The completed web cards #301 and #438 should be audited against the actual
FastAPI first-hour journey: typed request, validation error, reload, schema,
interactive docs, test, and production artifact. If those outcomes are absent,
create a successor rather than claiming web parity from transport support.

### Data analysis

Python wins current exploratory analysis. pandas offers broad tabular
operations and input/output formats, documented in its
[package overview](https://pandas.pydata.org/docs/getting_started/overview.html).
Python notebook tools currently provide the authoring, execution, inspection,
and plotting flow that Jet's first-party page lacks.

Jet's strongest alternative is not API-for-API imitation. It is checked column
selection, deterministic Core behavior, explicit missing-data policy, safe
parallelism, and a native pipeline artifact. Cards #237 and #307 should measure
the common transformations and the source cost against pandas and Polars. Until
then, "replace your Python data work" is not supportable.

### Tests

pytest turns plain `assert` statements into detailed failures and supplies
discovery and fixtures, as its
[getting-started guide](https://docs.pytest.org/en/stable/getting-started.html)
shows. Jet's integrated `#Test` avoids a dependency and can check language
effects, but pytest currently offers the smoother small-test authoring and
fixture experience.

Jet should match assertion introspection, temporary resources, parameterized
cases, filtering, and failure navigation through first-party tools. This is
mostly a tooling and library task, not a syntax task.

### Concurrency

Python offers concise high-level concurrency, but users must choose threads,
processes, or `asyncio`. Cancellation is subtle: Python warns that swallowing
`CancelledError` can break structured-concurrency components in the
[`asyncio` task documentation](https://docs.python.org/3/library/asyncio-task.html).
Python 3.14's free-threaded build is now officially supported but remains
optional and can require performance and extension work, according to
[PEP 779](https://peps.python.org/pep-0779/).

Jet's synchronous-looking task surface, explicit sharing, and data-race safety
are strong reasons to convert concurrent services. The cost is earlier
ownership thinking. Jet should demonstrate this with the same program growing
from serial to parallel, including cancellation and cleanup, and show which
Python failure modes become impossible.

### Packaging, dependencies, and supply chain

Python's packaging story has improved. `pyproject.toml` standardizes project
metadata and build configuration, and `pylock.toml` now defines a reproducible
lock-file format. Python still asks users to choose among build backends and
workflow tools; the official
[tool recommendations](https://packaging.python.org/en/latest/guides/tool-recommendations/)
do not prescribe one universal workflow. OS-managed Python installations add
another boundary described by
[PEP 668](https://peps.python.org/pep-0668/).

Jet's one-tool design can be materially easier, but the current registry publish
path does not upload, store garbage collection is a stub, and the documented
install begins with Nix. Cards #6, #423, #399, and #1095 must ship before Jet
claims a simpler end-to-end package workflow.

Jet should also avoid a stale caricature of PyPI. PyPI supports digital
attestations and Trusted Publishing. Those attestations prove publisher
identity and artifact integrity, not that code is benign, as PyPI's
[security model](https://docs.pypi.org/attestations/security-model/) explains.
Jet's differentiator should be enforced hashes, provenance, permissions,
capabilities, and audit output as one coherent workflow.

### Notebooks and exploration

Jet has a kernel and tested protocol/session machinery. The current first-party
web page is a status view with a title and cell counts. It has no interactive
cell editor or execution controls. One notebook session test also ends its
browser assertion with `|| true`, so that assertion cannot fail.

Python therefore wins the actual notebook journey. Card #442 is marked done,
but the user-facing artifact does not meet a Python converter's meaning of a
notebook. Reopen it or create a narrowly scoped successor for authoring,
execution, output inspection, plots, keyboard flow, and recovery.

### Deployment

Jet wins when it emits one native artifact that runs without a Python
interpreter, virtual environment, or application bundler. This is one of the
few conversion benefits that is immediate, concrete, and easy to demonstrate.
The pitch should show copying the binary into a clean environment and running
it. It should also show its provenance and permissions.

### Migration and dependency escape hatches

The shipped `jet import py` accepts a useful but narrow subset: annotated
top-level functions, simple scalar types, straight-line assignments, returns,
calls, arithmetic, comparisons, Boolean expressions, and equality assertions.
It does not provide a general Python application migration path.

General Python FFI is ratified but not yet a demonstrated, supported conversion
workflow. Cards #1155, #1156, #180, and #1125 are therefore central product
work. A converter must be able to:

- import or wrap one typed module;
- keep an unsupported Python library behind an explicit boundary;
- report every unsupported construct without silent semantic change;
- show copy, ownership, effect, exception, and concurrency behavior at that
  boundary;
- update generated code without overwriting user changes.

Dynamic reflection and monkey patching will not always translate. Jet should
say so clearly and make the foreign boundary excellent rather than recreating
Python's dynamic semantics inside Jet.

## Python lessons Jet should retain or avoid

| Python property or pain point | Jet status | Required action |
| --- | --- | --- |
| Very low first-draft ceremony | Jet loses in most small tasks | Compress safe common intent through inference and Core APIs. |
| Runtime does not enforce annotations | Avoided | Keep one semantic truth across compiler and tools. |
| Exceptions are an implicit effect | Avoided | Preserve typed errors; improve happy-path inference. |
| Aliasing, mutable defaults, and type-dependent mutation | Avoided by design | Teach the practical benefit in Python terms. |
| `None`, truthiness, and sentinel conventions | Mostly avoided | Keep explicit options and exhaustive matching compact. |
| Async coloring and subtle cancellation | Mostly avoided | Prove task cancellation and cleanup with matched examples. |
| Threading/runtime mode and extension ABI splits | Avoided for native Jet | Make foreign runtime mode and sendability explicit. |
| Packaging and environment choice overload | Promising design, not shipped | Finish install, registry, lock, provenance, and cleanup. |
| Import-time side effects and monkey patching | Mostly avoided | Do not add ambient extension mechanisms. |
| Excellent REPL, traceback, and pytest feedback | Jet trails | Invest in run, test, debug, and notebook feedback. |
| A selected Python dependency has no typed Jet bridge | Jet trails | Ship and test an audited bridge for representative dependencies. |
| Major-version migration pain | Risk remains for any language | Pair versioning rules with diagnostics, formatter, importer, and codemods. |
| Index selection and artifact provenance require explicit policy | Jet has a better intended model | Ship the enforcement before marketing it. |

The core lesson is not "Python is bad." Python optimizes discovery and
composition. Jet should preserve that directness while moving ambiguity from
runtime incidents into local, teachable choices.

## Conversion sellability

### Credible audiences after the immediate gates

- Python teams shipping internal CLIs and automation to many machines.
- Services where exception paths, cancellation, memory use, and deployment
  reproducibility matter.
- Data pipelines that value checked schemas and deterministic native execution
  more than notebook-first exploration.
- Teams moving performance-sensitive components out of Python but wanting a
  safer surface than C or C++.

### Audiences Jet should not target as converts yet

- notebook-first analysts and researchers;
- applications built around dynamic metaprogramming;
- teams whose essential libraries have no Jet implementation or typed bridge;
- beginners who only need a disposable local script;
- web teams expecting FastAPI-level schema, reload, and documentation on the
  first route.

### Claims supported now

- Memory- and type-safe by default.
- Mutation, ownership, and failure are source-visible; inferred effects have
  tool-visible provenance and optional explicit upper bounds.
- Structured tasks avoid a second async language.
- One source file can become a native artifact.
- Core includes first-party files, process, HTTP, data, CLI, and test surfaces.
- The compiler can teach at the point of error.

### Claims not supported now

- "Jet takes less code than Python."
- "Jet has typed compatibility with common Python dependencies."
- "Jet has notebook parity."
- "Jet has a simpler install and package journey today."
- "Every Jet script has an instant edit-run loop."
- "Jet replaces pandas, pytest, FastAPI, or PyPI."
- "`jet import py` migrates general Python applications."
- "Jet is a universal Python replacement."

The honest current pitch is:

> Jet keeps a direct scripting surface, then adds compile-time guarantees and a
> native delivery path. In today's matched automation corpus, that costs about
> 80–100% more source. The product goal is to reduce the cost to 25% or less
> while keeping the guarantees.

Do not market the sigils themselves. Market the outcomes: no accidental shared
mutation, no hidden exception channel, no unbounded body by default, no runtime
environment at deployment, and no async rewrite when work becomes concurrent.

## Ranked action backlog

### P0 — Conversion blockers

1. **Finish the shipped JSON execution path.** The `Json`/`DataTree` source
   mismatch is repaired and `jet check` passes. Close the remaining default-JIT
   E0956 and golden execution gap with the current encoding/golden owner rather
   than creating a parallel mechanism.
2. **Close the two run stories with user evidence.** Finish #688 and #666.
   Publish separate warm JIT, cold build, cached release build, and built-binary
   measurements. No fast-script claim may rely on a skipped release gate.
3. **Ship a no-Nix first install.** Close #399 and #1095 with a clean-machine
   Python-converter journey.
4. **Make the notebook a notebook.** Reopen #442 or create a successor for
   editable cells, execution, outputs, plots, keyboard use, and recovery.
   Remove the false-green `|| true` assertion.
5. **Prove the Python bridge and importer.** Close #1155, #1156, #180, and
   #1125 against a representative typed Python package and explicit
   unsupported-construct reporting.

### P1 — Surface parity

6. **Finish the matched workload corpus.** Close #769 and #1171 with HTTP,
   SQLite, CSV/data, typed CLI, tests, notebooks, and browser/process cases.
   Add paired change, repair, diagnostic, and refactor turns. Require an
   idiomaticity review for both languages.
7. **Set a source-cost budget.** Median Jet lexical tokens at or below 1.25
   times Python; no ordinary task above 1.5 times without a named guarantee
   exercised by the test.
8. **Close practical data gaps.** Use #237 and #307 to prove common select,
   filter, join, group, missing-data, CSV, and plot-preparation journeys.
9. **Audit first-route web parity.** Recheck #301 and #438 against validation,
   reload, schema, interactive docs, testing, and deployment. Add only missing
   user outcomes.
10. **Match small-test ergonomics.** Add first-party assertion
    introspection, parameterization, temporary resources, filtering, and
    failure navigation without a second test framework.
11. **Finish packaging as one workflow.** Close #6 and #423 with immutable
    upload, lock, provenance, permissions, offline restore, and garbage
    collection.
12. **Expand safe automation.** Close #772 and extend process evidence through
    pipes, streaming, signals, terminal use, cancellation, and backpressure.

### P2 — Conversion acceleration

13. **Publish a Python-to-Jet task cookbook.** Show the same practical program
    in both languages and explain only the extra guarantee each Jet line buys.
14. **Add Python-shaped diagnostics.** Detect common alias, mutable-default,
    unchecked-status, exception, optional-value, and async-cancellation
    migrations. Explain the Jet alternative at the error.
15. **Make the bridge a product escape hatch.** Document performance,
    ownership, exception, threading, security, and deployment behavior. Keep
    it explicit and auditable.
16. **Build a conversion benchmark dashboard.** Track source tokens, edit-run
    latency, diagnostics, test authoring, artifact size, and clean-machine
    deployment for every paired task.

Existing Tower cards own most work. Deduplicate against them before creating
successors. The JSON regression, notebook artifact gap, test false positive,
and first-route parity check are the only new concrete gaps identified here.

## Flip criteria

Jet can make a broad Python conversion claim only when all of these are true:

- At least 10–12 idiomatically reviewed task pairs cover CLI, files, processes,
  HTTP, web, SQLite, data, tests, notebooks, and browser automation. Paired
  change, repair, diagnostic, and refactor turns measure maintainability.
- Median Jet lexical tokens are at most 1.25 times Python, and no ordinary task
  exceeds 1.5 times without an exercised safety benefit.
- The release warm-run gate passes against the fastest installed peer, and
  cached release runs do not repeat a 14-second build.
- A new user installs Jet and runs hello in under five minutes without Nix
  knowledge.
- Every shipped feature example passes `jet check` and its executable golden;
  no golden path aborts the harness.
- `jet import py` converts a representative typed package, preserves updates,
  and reports every unsupported construct.
- A first-party notebook lets a user author, run, reorder, inspect, and plot
  from the browser.
- The Python bridge lets an application retain an essential package without
  hiding effect provenance, copies, exceptions, or runtime requirements.
- The resulting native artifact still deploys as one inspectable, reproducible
  unit.
- Every marketing claim links to the paired corpus evidence that proves it.

## Verification record

The audit ran:

- `scripts/agent/jet-env cargo build` — passed.
- `cargo test --test agent_workloads
  equivalent_adapters_complete_declared_tasks` — eight task cases passed for
  both Python and Jet.
- `cargo test --test run_cache script_start_budget_fixtures_and_peers` —
  passed; release hard gate skipped because `target/release/jet` was absent.
- `jet check` on representative CLI, data, concurrency, HTTP, process, and
  first-hour examples — passed.
- `jet check examples/features/serde/json.jet` — now passes after the
  example-quality correction.
- Corpus-wide example-quality verification is recorded in
  `docs/audits/example-quality-2026-07-26.md`.

This report changes no syntax and makes no owner-gated design choice.
