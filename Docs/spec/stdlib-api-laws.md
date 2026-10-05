# Core API ergonomic laws (D-STDRUBRIC1=A)

This specification is for Core API authors and reviewers. It defines the
naming, failure, ownership, effects, allocation, diagnostics, and evidence
rules for a public function, method, or type. The executable surface lives in
[`Core.jet`](../../crates/jet-codegen/src/Prelude/Core.jet) and the Jet source
under [`Core/`](../../Core/); accepted syntax and compiler-owned names live in
[`Syntax.rs`](../../crates/jet-foundation/src/Syntax.rs). The
[Core surface ledger test](../../tests/core_surface_ledger.rs) and its
[checker](../../Tools/agent/check-core-surface-ledger.mjs) provide focused
machine checks; runnable examples live under
[`Examples/features/`](../../Examples/features/). API names follow the
[Jet vocabulary](vocabulary.md) for streams, readers, and events.

These are review laws, not a second API catalog. A new declaration must pass
the laws even when an older declaration does not; existing drift is not an
exception.

At function review, ask two questions. Does the projected effect row plus the
signature tell the complete story? Does the body stay at one abstraction
level? Character-code work or a hand-written search inside a higher-level
helper belongs in a named lower-level brick. These prompts align the contract
with the implementation; they do not add a second mechanism.

## Law 1 — Naming

Use plain English words rather than abbreviations (`remove`, not `rm`). The
closed exception list is `len`, plus the module names `fmt`, `args`, and `mem`
(D-API-LEN1=A; a ballot is required to extend it).

Use `has(value)` and `has_key(key)` for membership predicates
(D-API-CONTAINS1=B). Use `add` for storage: keyed `add(key, value)` returns the
displaced value, keyed `add_new(key, value)` never overwrites and reports
whether it stored, and element `add(value)` reports whether it added a new
element (D-API-STORE1=A).

`List.remove(value)` removes the first equal value. Pass `.Slot` for positional
removal (D-LISTREMOVE1=F). Do not add parallel `remove_value` or `remove_at`
spellings. Prefix Boolean predicates (`is_empty`, `has_prefix`); use
`contains` when the job is content search. A failure-returning variant adds no
name suffix: its `!Error` contract signals failure.

Use these constructor forms (D-API-CTOR1=A):

- `Type(args)` when the arguments are the value's components;
- `Type.new(...)` for a fresh stateful container;
- `Type.over(x)` for a non-owning view over existing data;
- `Type.from_*(x)` for a conversion;
- `Type{ ... }` for a plain-data record (D-DOTCTOR1).

A new construction shape requires a ballot. Keep standard acronyms fully
capitalized under S66 (`JSONDecoder`, `HTTPClient`, `IOError`, `UTF8Error`);
do not add PascalCase aliases.

### Collection verb table (D-ONCE-VERB1=A)

This is the single review table for collection verbs. Reference pages and API
reviews may render it, but they must not create another verb list.

| Job | List | Map | Set | Queue | PriorityQueue |
| --- | --- | --- | --- | --- | --- |
| remove and return | `pop()` | `pop(key)` | `pop(value)` | `pop_front()` / `pop_back()` | `pop()` |
| swap at an index | `replace(index, value)` | — | — | — | — |
| store or upsert | — | `add(key, value)` | `add(value)` | — | — |
| membership | — | `has_key(key)` | `has(value)` | — | — |
| content search | — | `contains_value(value)` | — | `contains(value)` | — |

Text and Bytes also use `contains` for content search. Conversion naming uses
the same rule: bare `.from(source)` is the generic form; a source-qualified
conversion names its source, such as `.from_keys(keys, default)` or
`.from_bytes(bytes)`. Do not add a second bare or source-qualified spelling for
the same conversion.

## Law 2 — Fallibility

Return `T !E` when an operation can legitimately fail; do not panic for an
expected failure. For example, a parser returns a typed error rather than
using a sentinel or terminating the process. Reserve panics for programmer
errors such as indexing outside a known-size slice.

An error carries enough context for a useful diagnostic without source-code
inspection. Prefer the most specific error type available; use `Err` only as
the last resort for a heterogeneous set of failure paths.

## Law 3 — Ownership and allocation

A read-only function takes bare `T`; an unmarked read access never elevates.
Return a new allocation by value. Do not write into a caller-supplied buffer
unless the API is explicitly a low-allocation path.

Make mutation visible: a mutating function takes `&T`, while ownership
transfer takes `^T`. Document the invariant enforced by every `#SingleUse`
type.

## Law 4 — Effects

Declare I/O with the corresponding effect row, such as `-[FS]>`, `-[Net]>`, or
`-[Exec]>`. A pure function has no effect marker, and the compiler enforces
that boundary. A function that performs several effects lists all of them;
none may be hidden in an apparently pure signature.

Comptime eligibility uses the shared effect fact: an empty effect set is Tier 0,
and recorded Tier-1 inputs are locked for reproducibility.

## Law 5 — Allocation budget

When a hot-path allocation profile is not obvious, state it in a doc comment
(for example, “allocates one collection per call; prefer the iterator form for
large inputs”). Provide a streaming or iterator API beside a collect-to-list
shorthand.

Do not hide an unbounded allocation in a function that looks O(1). The caller
must be able to observe or bound the allocation.

## Law 6 — Diagnostics and fix hints

Every fallible API writes an explicit success/error contract (`T !E`, or `!E`
for a no-value success path) or deliberately uses the language's implicit
`Err` contract. Contextual propagation uses `?(text)` where a call-site reason
is needed. Each expected misuse has a corresponding UI snapshot showing the
message a user sees (I4).

Messages follow the voice and format in
[`diagnostics.md`](diagnostics.md): state what happened, why it happened, and
how to fix it. When a type or method is `#MustUse`, name the missed call in the
diagnostic.

## Law 7 — Examples

Give every new type a runnable example under `Examples/features/` with
golden-tested expected output (I5). Use plausible domain names rather than
`foo`, `bar`, or opaque placeholders. Show the successful path first and
failure handling second.

## Law 8 — One way to mean it (I8)

Before adding an API, search for an existing API that covers the same semantic
job. Extend or document that API instead of adding a second spelling. A
convenience shorthand is acceptable when it only composes existing primitives
and adds no behavior, such as `slice.first()` over an optional `slice[0]`
result.

## Law 9 — Sibling methods stay the house style (D-STDLIB-OPTPARAM1=A)

Use named sibling methods when variants make different semantic choices. Keep
each parameter list honest and make the common operation the short canonical
spelling. Do not add a mode parameter or preserve an alias for the same
operation.

```jet
&text.replace("a", "b")
text.replace_first("a", "b")
```

The codec matrix keeps genuinely different signatures visible in names such as
`read_i16_le` and `read_f64_be`. A callback replacement is a separate method
because its parameter and control shape differ.

## Signature-honesty review rows

| Row | Core API rule |
| --- | --- |
| Weakest-guarantee parameters | Ask only for the guarantee used: unmarked read access for reading, `Iterable` for iteration, and a view for a window. Do not require a concrete container when iteration is sufficient. If a strong guarantee is needed, require its proof type, such as a proven range, typestate, or unit, rather than a prose precondition. |
| Calculate/do split | Separate calculation from effects. Expose an effect-free sibling that returns data, accepts a callback, or returns an `Iter` when materialization is too expensive. The effectful convenience wraps that sibling and uses the same Prelude operation. |
| No hidden-lookup APIs | Take the value, not a name plus an ambient registry. Resolve a name at the caller, where the registry is explicit. |

### Print family (D-ONCE-PRINT1=A)

Give each spelling one job. Beginners learn ambient `print` first.

| Spelling | Job | Default or disposition |
| --- | --- | --- |
| `print(value, ...)` | Display each value and end each line. | Beginner default; no import. |
| `term.print(value, ...)` | The same one-line-per-value operation through `core.term`. | Qualified twin for `#NoPrelude` files. |

Interpolation builds a string. `:Debug` selects the existing debug
representation selector; it is not another print API.

## Review record

Record the real call sites, defaults, options audit, and required evidence in
the review system. This page supplies the laws; it does not replace a call-site
review.

| Field | Required record |
| --- | --- |
| Function/type | The declaration under review. |
| Ratified decision(s) | The applicable `D-...` or `S-...` citations. |
| Changed call sites | File, line, and the call read aloud. |
| Defaults rows | The matching rows below, or `none`. |
| D4 audit | Scope, result, and every exception. |
| Exception disposition | A ratified policy, or `none`. |
| Required evidence | Example, diagnostic snapshot, and focused proof. |

Use the following review questions. Their IDs are stable citations for review
receipts.

| ID | Review question |
| --- | --- |
| L1 | Naming is plain English, predicate-prefixed, and uses S66 acronyms. |
| L2 | Fallibility is in the return contract; panic is only for programmer error. |
| L3 | View, ownership, and mutation are explicit. |
| L4 | All access markers and effects are declared. |
| L5 | Non-obvious allocation is budgeted and the streaming form is present. |
| L6 | Every error path has the required diagnostic copy and UI snapshot. |
| L7 | A golden-tested example exists under `Examples/features/`. |
| L8 | No duplicate API or overload family remains. |
| L9 | Same-subject variants with the same result and safety shape use one root method with a safe default and a label-only option enum. Keep a sibling only when the parameter list, result shape, fallibility, or safety differs; name that distinct job plainly. |
| Weakest-guarantee parameters | The declaration asks for the weakest sufficient guarantee. |
| Calculate/do split | Pure calculation is separate from effectful convenience. |
| No hidden-lookup APIs | Registry lookup is explicit at the call site. |
| C1 | The actual call site is useful and was judged before the declaration. |
| C2 | Required values are positional; ambiguous or uncommon options are labeled. |
| C3 | Common dataflow reads left to right through methods and `?`. |
| D1 | The bare call performs the safest common operation without setup ceremony. |
| D2 | Every magic default has one row here and an explicit override. |
| D3 | Defaulted labeled options replace option-only overloads. |
| D4 | Every policy option is a dedicated enum; no Boolean or bare-string flag remains. |
| F1 | Expected failure is `T !E`, and contextual propagation is `?(text)`. |
| F2 | Every lookup returns `?T`; no sentinel, empty-status, or follow-up status check remains. |
| F3 | Every failure says what happened, why, and how to fix it. |
| T1 | Domain values use domain types while obvious beginner literals work at the boundary. |
| T2 | Core values are immutable; mutation belongs to containers. |
| T3 | Distinct concepts have distinct types and one simple entry door. |
| N1 | The name follows one subject-first grammar, including acronyms. |
| N2 | Pure and mutating operations use their systematic noun/past-participle and imperative pairs. |
| L-A | The I/O domain has both a whole-value call and its streaming seam. |
| L-B | Concrete containers are eager; `.lazy` uses the same adapter vocabulary. |
| L-C | A new container implements the one iteration protocol and inherits its adapters. |
| L-D | Beginner presets compose expert primitives instead of forking them. |
| E1 | Superseded spellings and implementations are deleted in the same greenfield cutover. |
| E2 | A measured gap-filler is absorbed with the useful wrapper defaults. |

The constructor and collection-verb rows remain governed by D-ONCE-VERB1; this
record does not reopen that reconciliation.

## Core rung splits

`D-ONCE-LAYER1=B` ratifies two taught rungs when one Core subject has a safe
default and an explicit control surface. `core.crypto` is the typed rung;
`core.crypto.expert` is the raw-byte rung. `core.http` is the one-shot rung;
`core.http.client` is the configurable rung. The compiler surface and a golden
example must cross-reference both doors and show the same operation through each
one.

## Core doctrine

The core-library slate ratifies these durable rules:

- **D-CORE-DOCTRINE1=A** — every new or changed Core API passes the Part A rules
  in review. Read call sites aloud, use enums for options, and keep one table
  of each magic default and its override.
- **D-CORE-EAGER1=A** — helpers on a real list, map, or set run immediately and
  return a plain collection. `.lazy` supplies the same vocabulary on a
  deferred view. Streams and file lines remain naturally lazy because they
  arrive over time.
- **D-CORE-PATH1=A** — `Path` is a prelude type with `join`, `parent`,
  `extension`, `stem`, `normalize`, `walk`, and the other path methods. Core
  functions that take a path accept a plain `String` or a `Path`; expert APIs
  may require `Path`. Path methods replace free `core.path` functions, and no
  path-join operator exists.
- **D-CORE-PRELUDE1=A** — the seven criteria are law: measured frequency, total
  and safe behavior, names that never change semantics, no better home,
  first-hour coverage, one fixed set, and collision-conscious names. User
  shadowing wins with a compiler warning, including `assert_eq`; the intrinsic
  comparison applies only when that prelude name is unshadowed. New names land only at epoch
  boundaries and use the L2001 migration lint for older packages. Every entry
  is total or returns a result; no implicit conversion enters the prelude.
  `Duration` and `Instant` are the Time-family quantities from
  D-TYPE2-TIME1.
- **D-CORE-PRELUDE2=B** — `read_file`, `write_file`, and `file_exists` belong to
  the prelude; random operations remain in `core.math.random`.
- **D-CORE-TREE1=A** — Core uses one nested tree. It keeps `core.files`, nests
  random under `core.math.random` and `fmt` under `core.text.fmt`, merges env
  and os into `core.sys`, and places terminal, process, and encoding surfaces
  in their canonical homes. JSON stays under `core.encoding.json`; a
  non-canonical free namespace has no alias or re-export.
- **D-CORE-USELIST1=A** — every grouped `use` uses square brackets. `as` gives
  a shorter local name; without `as`, the local name is the final component
  after the last dot. Existing brace item imports use the same list form.

## Extended Core API doctrine

Use this short table as the review index; examples and evidence remain in the
laws above.

| Rule | Test |
| --- | --- |
| C1 | Judge the call site, not the declaration. |
| C2 | Required values are positional; labels make uncommon or ambiguous options readable. Do not force `*` zones on simple APIs; reserve them for load-bearing names. |
| C3 | Common dataflow reads left to right through methods and `?`. |
| D1 | The bare call performs the safest common operation without setup ceremony. |
| D2 | Every magic default appears in the defaults table and has an explicit override. |
| D3 | Defaulted options replace overload families. |
| D4 | Options use dedicated enums; Core does not use Boolean or bare-string policy flags. |
| F1 | Expected failure is `T !E`; contextual propagation uses `?(text)`. |
| F2 | A lookup returns `?T`; sentinel values are not an API contract. |
| F3 | Every failure message states what happened, why, and how to fix it. |
| T1 | Domain values use domain types while beginner literals remain accepted at the boundary. |
| T2 | Core values are immutable; mutation belongs to containers. |
| T3 | Distinct concepts have distinct types and one simple entry door. |
| N1 | Names follow one subject-first grammar, including acronyms. |
| N2 | Pure operations use noun or past-participle names; mutating operations use imperative names. |
| L-A | Each I/O domain has a whole-value call over a streaming seam. |
| L-B | Concrete containers are eager; `.lazy` opts into the same deferred vocabulary. |
| L-C | New containers implement the one iteration protocol and inherit its adapters. |
| L-D | Beginner presets compose small expert primitives; they do not fork them. |
| E1 | A superseded spelling is deleted in the same greenfield cutover. |
| E2 | A measured gap-filler is absorbed with the wrapper's useful defaults. |

### Part A relation map

| Existing law | Part A rule(s) | Effect |
| --- | --- | --- |
| L1 naming | N1, N2 | Extends naming with a grammar test and side-effect pairs. |
| L2 fallibility | F1, F2 | Extends fallibility with a one-character test and sentinel ban. |
| L3 ownership | T2 | Unchanged: values remain immutable. |
| L4 effects | — | Unchanged. |
| L5 allocation | L-A, L-B | Adds the whole-value layer and eager default. |
| L6 diagnostics | F3 | Unchanged, restated. |
| L7 examples | — | Unchanged. |
| L8 one way | D3, L-D, E1 | Adds defaults instead of overloads and presets instead of forks. |
| New ground | C1–C3, D1, D2, D4, T1, T3, E2 | Adds these laws. |

### Worked review failures

These are doctrine examples: each shows a rejection, its reason, and a
reviewable replacement. They do not declare additional APIs.

#### D4: Boolean policy option

```jet
// Rejected: the call does not say what `true` selects.
parse(text, true)
```

A policy choice names its choice with a dedicated enum and a label:

```jet
parse(text, on_error: .Lenient)
```

A Boolean result, a Boolean data value, a Boolean enum payload, an
implementation parameter, and a compiler-only handle are not D4 policy
options. The audit covers user-facing policy and configuration choices only.

#### F2: Sentinel lookup result

```jet
// Rejected: the same Int is both an index and the "absent" signal.
find_or_minus_one(items, needle) // returns Int; -1 means absent
```

Carry absence in the type instead:

```jet
items.find(needle) // illustrative result: ?Item
```

The caller handles `None` as absence or propagates it through the ordinary
optional path. It does not compare a valid value with a sentinel or inspect a
second status result.

### Magic defaults and expert overrides

Use this table for the worked doors and option-bearing Core surfaces. APIs with
no magic default need no row. New entries extend this table or reuse an
existing option.

| Door | Bare default | Explicit control |
| --- | --- | --- |
| `files.read(path)` / `files.write(path, text)` | Whole-value file operations; normal writes use the safe path. | `open`, `create`, `append_all`, and labeled write modes. |
| `http.get(url)` | One-shot request through the safe HTTP door; HTTPS-to-HTTP downgrade is refused. | `core.http.client.session()` with `session_timeout`, `session_redirects`, `session_retries`, `session_proxy`, and opt-in `session_cookie`. |
| `http.client.session()` | 30,000 ms timeout, no retries, at most 10 redirects, no proxy, and an empty cookie jar. | The corresponding `session_*` methods. |
| `http.server.static_files(mux, prefix, root)` | Normalize the root, refuse escapes, hide dot-files, refuse escaping links, and serve `index.html` for a directory request. | `index`, `dotfiles`, and `follow_links` policy. |
| `http.server.cors_policy(origins)` | No CORS header exists until a policy is installed; unsafe origin/credential combinations are rejected. | `methods`, `headers`, `credentials`, and `max_age`. |
| `encoding.*.reader` / `encoding.*.writer` | Use bounded codec limits; JSON writing is non-canonical unless requested. | A `limits` value and JSON `canonical` choice. |
| `list.map` / `list.filter` | Eager plain collection. | `.lazy` for a deferred view. |
| `time.now` | Current Unix time from the ambient standard clock. | Inject a `Clock`, including `core.testing.fake_clock(unix_ms)`, for deterministic code. |
| `crypto` | Typed safe values and fail-closed defaults. | `core.crypto.expert` inside the audited raw-byte boundary. |

## Competitive Core API gate (D-STDRUBRIC1=A, card #1398)

This is the Core API superiority gate. Python is the calibration arm, but the
comparison covers every language recorded in the Core surface ledger.

The workflow inventory is generated on demand by
[`check-core-surface-ledger.mjs`](../../Tools/agent/check-core-surface-ledger.mjs).
`--check` rebuilds `.jet/reports/core-surface-ledger.json` from compiler and
source tables and never reads that report. Every generated row requires one
`coreApiGate.workflowManifest` entry.

### Frozen task record

Before comparison, record these fields for every workflow:

| Field | Required content |
| --- | --- |
| `task` | Stable task identity and ledger-row link. |
| `input` | The same input for every language arm, with fixture status frozen before the run. |
| `outcome` | The same semantic result, exit behavior, and normal-language contract. |
| `allowedDependency` | The standard-library or shipped-Core boundary; required imports count. |
| `toolVersions` | Pinned competitor and runner versions. |
| `sourceBoundary` | User-authored source paths; generated output, expected output, and reference source are excluded. |
| `competingCoreWorkflow` | The exact workflow named by the ledger row. |
| `cases` | Applicable `beginner`, `expert-policy`, `failure`, and `lifecycle` arms. |

Keep a design decline in the manifest as a scored loss. Only a ratified
product-scope decision may set `scope.excluded` to true.

### Score record

Each matched task reports:

- raw source counts for every arm, including imports, required policy, and
  required error handling;
- mandatory concept IDs, hidden facts, and nonlocal lookups;
- every extra Jet construct, classified as `task-essential`,
  `clarity-bearing`, `guarantee-bearing`, `expert-control`, or
  `incidental-ceremony`;
- the extra construct's span and source cost, one or more claimed
  `claimedClarity`, `reasoningBenefit`, `localFactBenefit`,
  `guaranteeBenefit`, or `expertControlBenefit` fields, the rejected shorter
  form, lost value, and reviewer verdict;
- the measured reasoning burden; a worse burden needs a compensating product
  win in the same evidence record;
- at least one measured or independently reviewed Jet win;
- independent acceptance that each competing fixture is idiomatic and minimal
  for the same task, input, outcome, and normal-language contract.

A measured record is not a bag of placeholders. `rawSourceCounts` has
`unit: "lexical-token"`, `includes: ["imports", "required-policy",
"required-error-handling"]`, and one `arms` entry per compared source. Each
arm reports non-negative `tokens`, `statements`, `calls`, and
`namedTemporaries`. `mandatoryConceptIds`, `hiddenFacts`, and `nonlocalLookups`
are arrays, including an explicit empty array when a fact or lookup is absent.
The fixture review records its reviewer, fixture task, same-task/input/outcome
check, idiomatic/minimal verdict, normal-language contract, and either
`pythonGuarantees: "not-emulated"` or `"not-applicable"`. A Jet win records a
property, evidence reference, kind (`machine` or `review`), and its measurement
or independent review. A record marked `pending-fixture` cannot pass the
release check.

Raw counts are evidence, not a universal ratio. An increase passes only when it
improves clarity, local reasoning, a named guarantee, or expert control.
Incidental ceremony fails. A worse reasoning burden needs a compensating
product win. Python does not imitate a Jet-only guarantee.

Use structured evidence and an independent review for readability and
reasonability. Use machine measurements for runtime, memory, artifact, safety,
diagnostic, bounds, and audit properties. The gate fails on stale fixtures,
unexplained ceremony, missing evidence, a missing Jet win, or an unowned loss;
every failure names card `#1398` as the release-gate owner.

The gate reuses the existing agent corpus manifest, receipt, runner, and `#769`
scoring contract. It adds no benchmark runner and no second scoring model.
Record independent fixture acceptance in
[`core_api_fixture_reviews.tsv`](../../tests/agent_workloads/core_api_fixture_reviews.tsv),
with one row for each frozen task. Bind each row to the adapter-source, input,
and expected-output digests. A Python row says `not-emulated` for Jet-only
guarantees. Reuse the runner's pinned tool versions, cold and warm executions,
exact stdout, unchanged-input, and clean-scratch checks; also verify stored
stdout/stderr artifacts and checksum closure. A fresh-context review records
closure, construct classifications, reasoning evidence, syntax coverage, and
fixture selection. A drift in any binding fails the gate under card `#1398`.

~~~sh
node Tools/agent/check-core-surface-ledger.mjs --check
node Tools/agent/check-core-surface-ledger.mjs --core-api-release-check
~~~

`--check` proves the source-derived inventory and frozen record shape. The
release check requires complete measured evidence and an accepted Jet win for
every manifest entry.
