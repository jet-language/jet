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

### Rust-exit accounting

[`Tools/agent/rust-leaves.tsv`](../../Tools/agent/rust-leaves.tsv) is the
checked registry for remaining Rust-backed Core dispatchers (#4742).
Every retained dispatcher requires a stable row identity and a reviewed hash
of its dispatch expression, even when a Jet source wrapper shadows it or
the generated/implicit runtime body is not yet mapped. Receiver identities
include the type list and method, not a moving declaration line number.

`leaf` records migrate the former CorelibInteg journals, preserving removal
conditions and provenance; they do **not** establish that a branch has landed,
that a leaf is policy-free, or that its behavior is proved. `port` records
track the other retained dispatchers. Neither kind grants Jet census credit.
A `-` hash records an unlanded leaf and cannot authorize a live dispatcher.
Changing a dispatcher requires reviewing its registry hash; retiring it means
removing the Rust provider, not merely hiding it behind a source-owned wrapper.

Lane checks run `node Tools/agent/rust-exit.mjs --check`. The A–H table in
[`rust-exit-ratchet.json`](../../Tools/agent/rust-exit-ratchet.json) partitions
physical Rust source lines (including blanks and comments), with the original
RUST-EXIT inventory snapshot retained separately. Inventory cross-references
overlap; area totals do not. Rust source symlinks are not counted a second time.
Build scripts belong to H before other area classifications; new nonignored
Rust source is counted before staging. Emitted/build/scratch output is excluded
by the repository ignore rules.

After a port, lower the ceilings with
`node Tools/agent/rust-exit.mjs --update`. This command refuses **any** increase
and does not modify the registry. A zero ceiling permanently excludes Rust
from that area. Run `node Tools/agent/rust-exit.mjs --check` for the focused
inventory gate and `node --test Tools/agent/rust-exit.test.mjs` for the planted
unregistered-leaf and one-line-growth refusal fixtures.

## Law 1 — Naming

Use plain English words rather than abbreviations (`remove`, not `rm`). The
closed exception list is `len`, plus the module names `fmt`, `args`, and `mem`
(D-API-LEN1=A; a ballot is required to extend it).
Component Model plugin export names use canonical kebab-case. A Jet method
spelling such as `is_enabled` resolves to the published `is-enabled` export;
generated guest exports and host invocation must use the same conversion.

Use `has(value)` and `has_key(key)` for membership predicates
(D-API-CONTAINS1=B). Use `add` for storage: keyed `add(key, value)` returns the
displaced value, keyed `add_new(key, value)` never overwrites and reports
whether it stored, and element `add(value)` reports whether it added a new
element (D-API-STORE1=A).
For `Tally`, adding another occurrence always stores it and returns `true`,
including when the element already has a nonzero count.

The result law also applies to ordered sets (`Rank`) and bit sets (`Bits`).
Shared runtime entry points retain the checked element carrier for typed
pointer writes; fixed-width values widen when the pointer stores exact `Int`.
Bounded native collection capacities and bit indexes convert exact `Int`
only at the shared runtime boundary, with a checked range failure rather than
an unchecked cast. Read-only keys and byte buffers are borrowed at that boundary.
List-like indexing also accepts fixed-width integers. Every integer carrier
crosses into the host index with a checked conversion; an unsigned index above
the host range stops with the same arithmetic failure as an oversized exact
`Int`, rather than wrapping into a negative index.

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

### Internal Core Shared publication

Core source can retain an existing `Shared<T>` owner with
`core.mem.__process_shared(init)`, `__world_shared(init)`,
`__thread_shared(init)`, or `__attempt_shared(init)`. These are reserved
Core-only operations, not public constructors. The same-module initializer
must be a direct, non-generic, zero-argument, infallible module function
returning `Shared<T>`; its inferred transitive effects may contain only `Mem`.
Every use of one initializer must select the same lifetime.

Sema publishes that checked identity and lifetime. Lowering creates one typed
MIR `Global` site per initializer, not a special initializer-name table or a
per-caller cache. A getter returns an owning clone; retiring a site releases
its owner without invalidating outstanding clones. Process sites survive
world reloads, World sites retire at an explicit quiescent world boundary,
Thread sites survive session/lease changes until their OS thread retires, and
Attempt sites retire after the selected ordinary unit attempt has discharged
its tasks and guards, including failure/unwind paths.

All native site cells belong to one pinned Core image. Dynamic images import
its getters and never own or register cells; the host must execute the image's
no-argument `Unit` startup, which performs frozen-static fixups only, before
publishing getters. Process/World first publication calls the canonical
runtime `jet_rt_shared_site_publish`; the common read is an ordinary typed
`Global` load. Thread/Attempt use host TLS projection of the same site identity.
Source evaluation receives a host-retained typed process segment and explicit
boundary resets, never a compiler-global root. The browser continues to support
ordinary `Shared`; only existing env/auth/fault APIs without browser providers
are refused in sema with `E-WEB-TIR-UNSUPPORTED`.

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
- **Stdio source representation** — `Stdout` and `Stderr` retain their declared
  public `fd: Int` fields on every tier. Factories return `1` and `2`; record
  construction, display, debug, and equality use those fields. The field is
  informational: even a user-constructed token with another value writes to
  its named process stream. Jet owns each public operation's fault checkpoint,
  error context, and flush decision. Nonempty writes and line writes flush;
  an empty write flushes only on a terminal. Write and its selected flush share
  one held writer lock, and both failures keep the original `Write` context.
  Explicit `flush` uses one `IO.Flush` checkpoint and a `Flush` context.
- **Stdin source representation** — `StdinHandle` retains its public `fd: Int`;
  both `stdin()` and `buffered()` produce `0`. The field is informational:
  constructed tokens still read the one process-wide buffered stdin and do not
  close its borrowed descriptor. `read_line` checks `IO.Read` once, preserves
  typed read errors, distinguishes EOF from an empty line, and removes every
  trailing CR or LF in source.

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

### Resident environment column transport

The interpreter and resident host bind the canonical `core.sys.__env_columns`
and `__startup_env_columns` leaves, not the retired count/name/value transport.
Logical columns come from one snapshot under the existing logical-cell lock;
startup columns cache immutable host `vars_os` separately. Both preserve
platform encoded name/value bytes and let source `env_columns` pair them.
The key comparison reuses Windows ordinal comparison and byte ordering on
other hosts. Neither bridge writes the operating-system environment.

### Realtime cancellation access

`RealtimeStream.cancel(&self)` forwards its write-borrowed stream to
`core.rt.__cancel(&self)`. The public method and its provider call both retain
the explicit write-access marker; cancellation does not consume the stream.

## Testing evidence source policy

`core.testing.golden` and `fixture` own success/failure selection and exact
evidence messages in `Core/testing/testing.jet`. Golden diffs compare at most
64 KiB and 256 inclusive-newline lines per input; their delete-first LCS tie
rule and UTF-8-safe 32 KiB output limit preserve the existing native policy.
The host supplies only file-read facts, failure TLS transport, and selected
string byte windows. These leaves do not add checkpoints to the old testing
operations.

`temp_dir` owns Unicode-scalar prefix sanitation, ASCII-only thread sanitation,
the `jet_test_` name, remove-before-create ordering, NotFound handling, and
error wording. Filesystem facts and actions remain raw host leaves returning
private plain records, never tuples. `__TempPath.path` is opaque platform
`OsString::into_encoded_bytes()` output: source must not construct, slice,
edit, or decode it, and passes it unchanged to remove/create. The displayed
lossy String is separate, so non-UTF-8 temporary roots retain their original
filesystem spelling. The native borrow is same-platform and allocation-free.
`Fake` owns its state, locale, tables, rendering, and deterministic choices,
sharing the existing public source RNG's seeded draw helper rather than a
second RNG identity.
`test_suite` snapshots the existing native harness state. `histories` owns
the three copy-before-callback wrappers; the existing engine still owns
history execution and compiler-derived command provenance.

I9 preserved gaps: the interpreter does not observe native testing-failure
TLS, and the native-only suite snapshot remains on its existing tier support.
Testing `world` is **native and uncredited**: exact interpreter diagnostic-error
cleanup requires an unavailable capture/rethrow rail, so its existing
callback/handle-unregister/wait/close/scope-restore order is unchanged.
## Source Fake clean cutover

The private state and locale fields, seed-bit conversion, exact locale salt,
fixed draw order, tables, Display, Debug, clone, and equality belong to
`Core/testing/testing.jet::Fake`. The seeded helper is the existing source
`core.math.random.__seeded_int`, not another RNG or native Fake carrier.

Removed native routes → source replacements (reviewed pre-removal line anchors):

- `Context.rs:1218`, `Emit/Types.jet:2665`, `core_types.rs:277`,
  `Traits.rs:2702` → source nominal Fake and ordinary record traits.
- `Collections.rs:558,2103,2908,3890,3952` and
  `method_calls.rs:6290` → source Fake methods and ordinary parameter/borrow rules.
- Rust `TIR/mod.rs:14242`, `routes.rs:2078`, `subset/handles.rs:248,561`,
  `tir_to_mir_expr.rs:4506`; Jet `TIR.jet:948`, `HandleMethods.jet:1043,1355`
  → source locale/name/email/host/address bodies, no closed Fake handle operations.
- Comptime `Builtins.rs:1438`, `dispatch/eval_method.rs:2355`,
  `Methods/core_calls.rs:378`, `plain_calls.rs:1243` → ordinary source dispatch.
- `Core.jet:414`, `CoreHostSignatures.jet:191`, `fixed_sigs.rs:1864`,
  `Codegen/mod.rs:3865`, native `FakeData.rs:1-69`,
  `CommonTypes.rs:2822,6819` → source constructor, fields, rendering and tables.
- `Syntax/core_surface.rs:996` → source type identity, no builtin Fake marker.

The Jet Foundation collection/trait registries and Jet Sema builtin-owner
registry had no Fake-specific path; they intentionally remain unchanged.
Generated C-ABI/route views remain frozen until the E2 final artifact permits
central regeneration. Receipt-2 temporarily restores the Rust Fake carrier,
its Clone/PartialEq/JetShow support, the six FakeData functions and their
helpers, and the Prelude include verbatim from `459cfe7cd^`, solely to satisfy
references in those frozen generated files. Core source ownership, registry
rows, and compiler/comptime dispatch remain cut over. This dead generated-route
glue dies at the post-freeze regeneration; it is not a source compatibility path.

## Browser resident adapter cleanup

The unused `BrowserHost::eval_core_call` plain-call dispatcher is removed,
along with its orphan-only `profile_value`, `profile_ct`, `timeout_ct`, and
`locked_ct` conversions. Repository-wide binding, qualified-call, string, and
macro searches found no callers. Unrelated compiler/MIR `eval_core_call`
functions are not that binding. The resident method adapter, handle table,
live timeout/locked conversions, and included AOT providers are unchanged.

### Realtime receipt presentation

`Core/rt/rt.jet::RealtimeReceipt` owns `render_json`, `render`, and
`frames_per_buffer`. The first two preserve the eight native receipt fields'
fixed labels, order, punctuation and whitespace (`Realtime.rs:119-144`
before removal); the last returns `requested_frames` (`Realtime.rs:147-149`).
All three read the existing receipt without effects or new host leaves.
The provider still owns bounded receipt observations and stream lifetime.
