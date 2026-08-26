# Core API ergonomic laws (D-STDRUBRIC1=A)

Review rubric for all Core API additions. Every new function, method, or type
must pass each law before landing. No exceptions; file a follow-up card for any
existing drift found during review.

The talks add two review questions: is this function honest—does its projected
effect row plus signature tell the complete story? Does its body stay at one
level of abstraction—if it zooms into character codes or hand-rolls a search,
that work belongs in a named brick? These are review prompts, not new
mechanisms; I8 is unchanged. The [2026-08-21 function-design canon mining
report](../reference/prior-art.md#function-design-canon) records the Logan
Smith video and its three linked sources.

---

## Law 1 — Naming

- Names are plain English words, not abbreviations (`remove`, not `rm`).
  Blessed exceptions (closed list, D-API-LEN1=A; ballot to extend): `len`,
  and the module names `fmt`, `args`, and `mem`.
- Membership predicates are `has(value)` / `has_key(key)` (D-API-CONTAINS1=B).
- Storage uses `add`: keyed `add(key, value)` returns the displaced value, keyed
  `add_new(key, value)` never overwrites and returns whether it stored, and element
  `add(value)` returns whether it added a new element (D-API-STORE1=A).
- `List.remove(value)` removes the first equal value by default; pass `.Slot` for
  positional removal (D-LISTREMOVE1=F). Do not add parallel `remove_value` or
  `remove_at` spellings.
- Boolean predicates are verb-prefixed: `is_empty`, `has_prefix`, `contains`.
- Failure-returning variants add no suffix; the `?` return type signals failure.
- Constructor idioms (D-API-CTOR1=A): bare `Type(args)` when the args are the value's
  components; `Type.new(…)` for fresh stateful containers; `Type.over(x)` for non-owning
  views over existing data; `Type.from_*(x)` for conversions. `Type.{ }` stays the
  literal for plain data records (D-DOTCTOR1). New construction shapes need a ballot.
- Standard acronyms stay fully capitalized per S66 (`JSONDecoder`, `HTTPClient`,
  `IOError`, `UTF8Error`). Do not add PascalCase aliases.

### Collection verb table (D-ONCE-VERB1=A)

This table is the one review truth row for collection verbs. Reference docs and
future API reviews render this row; they do not create a second verb list.

| Job | List | Map | Set | Queue | PriorityQueue |
| --- | --- | --- | --- | --- | --- |
| remove and return | `pop()` | `pop(key)` | `pop(value)` | `pop_front()` / `pop_back()` | `pop()` |
| swap at an index | `replace(index, value)` | — | — | — | — |
| store or upsert | — | `add(key, value)` | `add(value)` | — | — |

Conversion naming follows the same law: bare `.from(source)` is the generic
conversion form; a source-qualified conversion names its source, such as
`.from_keys(keys, default)` or `.from_bytes(bytes)`. Do not add a second bare
or source-qualified spelling for the same conversion.

## Law 2 — Fallibility

- A function that can legitimately fail returns `T E!`; never panics on expected failure.
- Panics are reserved for programmer error (index out of bounds on a known-size slice).
- The error type must carry enough context to write a helpful error message without
  inspecting source code (no opaque integer codes).
- Use the most specific error type available; `Err` (the default error) is a
  last resort for heterogeneous error paths.

## Law 3 — Ownership / allocation

- Functions that only read a value take bare `T`; unmarked read access is enforced
  and never elevates.
- Functions that return a new allocation return by value; they do not write into a
  caller-supplied buffer unless the API is explicitly a low-allocation path.
- Mutation is visible: a function that mutates a value takes `&T`; ownership
  transfer takes `^T`.
- `#SingleUse` types must be documented with the invariant they enforce.

## Law 4 — Effects

- I/O effects are declared with the right effect row (`-[FS]>`, `-[Net]>`,
  `-[Exec]>`, etc.).
- Pure functions carry no effect markers; the compiler enforces this.
- A function that performs multiple effects lists all of them; no hidden IO.
- Comptime eligibility follows the shared effect fact: an empty effect set is
  Tier 0, and recorded Tier-1 inputs are locked for reproducibility.

## Law 5 — Allocation budget

- Hot-path functions note their allocation profile in a doc comment when non-obvious
  (e.g. "allocates one Vec per call; prefer the iterator form for large inputs").
- Streaming/iterator APIs are provided alongside any collect-to-list shorthand.
- No function silently allocates unboundedly without the caller being able to observe
  or bound it (no hidden `collect()` inside an apparently O(1) function).

## Law 6 — Diagnostics and fix hints

- Every fallible function that returns a suffix-zone type (`T? E!`, `E!`, or bare `!`) has at least one corresponding UI snapshot
  showing the error message a user sees when misusing it (I4).
- Error messages follow the voice and format in `docs/spec/diagnostics.md`:
  what happened, why it happened, how to fix it.
- When a type or method is `#MustUse`, the diagnostic names the missed call.

## Law 7 — Examples

- Every new type has at least one runnable example in `examples/features/` with
  golden-tested expected output (I5).
- Examples use real-world plausible names (not `foo`, `bar`, `x`).
- Examples show the happy path first, error handling second.

## Law 8 — One way to mean it (I8)

- Before adding a new API, search for an existing one that covers the same semantic
  job. If one exists, extend or document it; do not add a second spelling.
- Convenience shorthand methods are acceptable if they compose existing primitives
  without adding new behavior (e.g. `slice.first()` over `slice[0]?`).

## Signature-honesty review rows

| Row | Core API rule |
| --- | --- |
| Weakest-guarantee parameters | Ask only for the guarantee the function uses: unmarked read access for reading, `Iterable` for iteration, and a view for a window. Never require a concrete container when iteration is enough. If a strong guarantee is needed, require its proof type, such as a proven range, typestate, or unit, instead of a prose precondition. |
| Calculate/do split | Separate calculation from effects. Expose an effect-free sibling that returns data, takes a callback, or returns an `Iter` when materializing costs too much. The effectful convenience wraps that sibling and uses the same Prelude operation. |
| No hidden-lookup APIs | Take the value, not a name plus an ambient registry. Resolve a name at the caller, where the registry is explicit. |

### Print family (D-ONCE-PRINT1=A)

The family has one job per spelling. Beginners learn ambient `print` first.

| Spelling | Job | Status and default |
| --- | --- | --- |
| `print(value, ...)` | Display each value and end each line. | Beginner default; no import. |
| `term.print(value, ...)` | The same one-line-per-value print through `core.term`. | Qualified twin for `#NoPrelude` files. |
| `term.println(value)` | No distinct job from `term.print`. | Retired; `jet fmt` and `jet fix` rewrite it to `term.print`. |
| `term.sprint(value)` | No distinct job from interpolation. | Retired; `jet fmt` and `jet fix` rewrite it to `"{value}"`. |
| `term.repr(value)` | No distinct job from debug interpolation. | Retired; `jet fmt` and `jet fix` rewrite it to `"{value:Debug}"`. |

Interpolation is the string-building mechanism. `:Debug` selects the existing
debug representation selector; it is not a second print API.

---

## Review template

When submitting a new or changed Core API for merge, copy this checklist into
the review record. It is self-contained: a reviewer records the real call
sites, the defaults rows, the options audit, and the drift cards here. No
proposal lookup is required.

```
## Core API review

Function/type: `<name>`
Ratified decision(s): <D-XXX / S-YYY>
Changed call sites: <file:line and the call read aloud>
Defaults rows: <table rows or `none`>
D4 audit: <scope, result, and every exception>
Drift cards: <card number for every existing exception, or `none`>
Required evidence: <example, diagnostic snapshot, and focused proof>

- [ ] `L1` Naming is plain English, predicate-prefixed, and uses S66 acronyms.
- [ ] `L2` Fallibility is in the return type; panic is only for programmer error.
- [ ] `L3` View, ownership, and mutation are explicit.
- [ ] `L4` All access markers and effects are declared.
- [ ] `L5` Non-obvious allocation is budgeted and the streaming form is present.
- [ ] `L6` Every error path has the required diagnostic copy and UI snapshot.
- [ ] `L7` A golden-tested example exists under `examples/features/`.
- [ ] `L8` No duplicate API or overload family remains.
- [ ] `Weakest-guarantee parameters` pass.
- [ ] `Calculate/do split` passes.
- [ ] `No hidden-lookup APIs` pass.
- [ ] `C1` The actual call site is useful and was judged before the declaration.
- [ ] `C2` Required values are positional; ambiguous or uncommon options are labeled.
- [ ] `C3` The common dataflow reads left to right through methods and `?`.
- [ ] `D1` The bare call performs the safest common operation with no setup ceremony.
- [ ] `D2` Every magic default has one row here and an explicit override.
- [ ] `D3` Defaulted labeled options replace option-only overloads.
- [ ] `D4` Every policy option is a dedicated enum; no Boolean or bare-string flag remains.
- [ ] `F1` Expected failure is `T E!`, and propagation is one `?`.
- [ ] `F2` Every lookup returns `T?`; no sentinel, empty-status, or follow-up status check remains.
- [ ] `F3` Every failure says what happened, why, and how to fix it.
- [ ] `T1` Domain values use domain types while obvious beginner literals work at the boundary.
- [ ] `T2` Core values are immutable; mutation belongs to containers.
- [ ] `T3` Distinct concepts have distinct types and one simple entry door.
- [ ] `N1` The name follows the one subject-first grammar.
- [ ] `N2` Pure and mutating operations use their systematic noun/past-participle and imperative pairs.
- [ ] `L-A` The I/O domain has both a whole-value call and its streaming seam.
- [ ] `L-B` Concrete containers are eager; `.lazy` uses the same adapter vocabulary.
- [ ] `L-C` A new container implements the one iteration protocol and inherits its adapters.
- [ ] `L-D` Beginner presets compose expert primitives instead of forking them.
- [ ] `E1` Superseded spellings and implementations are deleted in this change.
- [ ] `E2` A measured gap-filler is absorbed with the useful wrapper defaults.
```

The constructor and collection-verb rows remain governed by D-ONCE-VERB1; this
checklist does not reopen that reconciliation.

---

## Known drift (follow-up cards)

This is the current audit inventory, not a grandfather list. A row can point to
a completed owner card when that card shipped the surface but did not reconcile
the later doctrine rule. Such a row remains drift until a later decision and
implementation close it.

| Gap | API | Law | Follow-up |
|-----|-----|-----|-----------|
| Layering split between the typed/raw and one-shot/configurable rungs | `core.crypto.expert`, `core.http.client`, `core.time`, `core.mem` | `L-A`, `L-D` | #1725 |
| Boolean client policy controls (`protocols`, `allow_http_downgrade`, and `same_origin_credentials`) | `core.http.client` / `HTTPRedirectPolicy.Follow` | `D4` | #301, #1725 |
| Boolean static-file policy controls (`index`, `dotfiles`, `follow_links`) | `core.http.server.static_files` | `D4` | #1273 |
| Boolean CORS policy control (`credentials`) | `core.http.server.cors_policy` | `D4` | #1273 |
| Boolean encoding policy controls (`canonical`, `require_canonical`, `comments`, and edition-gated `allow_*` flags) | `core.encoding` | `D4` | #712 |
| Three Boolean regex policy flags | `core.regex.flags` | `D4` | #1471 |
| Boolean socket policy setter | `core.net.set_nodelay` | `D4` | #300 |
| Empty-byte lookup result on missing or invalid archive entry | `core.archive.tar_get` | `F2` | #1470 |
| Empty-string absence results and process-status sentinels | `core.url` accessors and `core.sys` facts | `F2` | #1472, #1465 |

Evidence boundary for this inventory: the 2026-08-14 scan covered the current
Core declarations in `crates/jet-sema/src/Sema/CheckerCoreLib`, the matching
Prelude surfaces, and `docs/reference/core-library.md` plus the ratified
encoding scope in `docs/spec/encoding-decisions.md`. Boolean results, data
fields, predicates, implementation parameters, and constructor sentinels are
not D4 options. The rows above are the policy and lookup exceptions found in
that scan; every exception has a card home and none is approved by this table.

Resolved disposition, #1691: `core.crypto.expert` now uses distinct
`x25519_raw` / `hkdf_sha256_raw` names, and
`examples/features/crypto/crypto_migration.jet` covers the audited raw path
while the safe `core.crypto` APIs retain typed defaults.

### Core rung splits

`D-ONCE-LAYER1=B` ratifies two taught rungs when one Core subject has a safe
default and an explicit control surface. `core.crypto` is the typed rung;
`core.crypto.expert` is the raw-byte rung. `core.http` is the one-shot rung;
`core.http.client` is the configurable rung. Each pair keeps a cross-reference
in the compiler surface and a golden example that shows the same operation
through both doors.

**2026-08-06 — the core-library slate: D-CORE-DOCTRINE1=A,
D-CORE-EAGER1=A, D-CORE-PATH1=A, D-CORE-PRELUDE1=A, D-CORE-PRELUDE2=B,
D-CORE-TREE1=A, D-CORE-USELIST1=A** *(card #1495, proposal
`docs/proposals/corelib-overhaul.md`)*. The tree migration itself lands with
card #1574. The reference doc restructure rides that cutover.

- **D-CORE-DOCTRINE1=A** — all Part A rules become law. Every new or changed
  Core API must pass them in review. Call sites are judged by reading them
  aloud, options are enums, and one docs table lists each magic default and its
  override.
- **D-CORE-EAGER1=A** — helpers on a real list, map, or set run at once and
  return a plain collection. `.lazy` gives the same vocabulary as a deferred
  view. Streams and file lines stay naturally lazy because they arrive over
  time.
- **D-CORE-PATH1=A** — `Path` is a real prelude type with `join`, `parent`,
  `extension`, `stem`, `normalize`, `walk`, and the other path methods. Every
  Core function that takes a path accepts a plain `String` or a `Path`. Expert
  APIs may require `Path` alone. The methods replace the `core.path` free
  functions; no path-join operator exists.
- **D-CORE-PRELUDE1=A** — the seven criteria become law: measured frequency,
  total and safe, names never semantics, no better home, first-hour coverage,
  one fixed set, and collision-conscious names. User shadowing wins with a
  compiler warning. New names land only at epoch boundaries and use the L2001
  migration lint for older packages. Every entry is total or returns a result;
  no implicit conversion enters the prelude. `Duration` and `Instant` are the
  Time-family quantities from D-TYPE2-TIME1.
- **D-CORE-PRELUDE2=B** — `read_file`, `write_file`, and `file_exists` join the
  prelude. Random stays in `core.math.random`.
- **D-CORE-TREE1=A** — Core uses a consistent nested tree. It keeps
  `core.files`, nests random under `core.math.random` and `fmt` under
  `core.text.fmt`, merges env and os into `core.sys`, and splits terminal,
  process, and encoding surfaces into their canonical homes. It does not add
  `core.json` or retain retired free namespaces.
- **D-CORE-USELIST1=A** — every grouped `use` uses square brackets. `as`
  gives a shorter local name; without `as`, the local name is the last part
  after the final dot. Existing brace item imports move to the same list.

## Extended Core API doctrine

The ratified Part A rules are the review test for every changed Core call. The
short form below is the current checklist; the proposal contains the evidence
and examples.

| Rule | Current test |
|---|---|
| C1 | Judge the call site, not the declaration. |
| C2 | Required values are positional; labels make uncommon or ambiguous options readable. Do not force `*` zones on simple APIs; reserve them for load-bearing names. |
| C3 | The common dataflow reads left to right through methods and `?`. |
| D1 | The bare call performs the safest common operation with no setup ceremony. |
| D2 | Every magic default appears in the defaults table and has an explicit override. |
| D3 | Defaulted options replace overload families. |
| D4 | Options use dedicated enums; Core does not use boolean or bare-string policy flags. |
| F1 | Expected failure is `T E!`; propagation costs `?`. |
| F2 | A lookup returns `T?`; sentinel values are not an API contract. |
| F3 | Every failure message states what happened, why, and how to fix it. |
| T1 | Domain values use domain types while beginner literals remain accepted at the boundary. |
| T2 | Core values are immutable; mutation belongs to containers. |
| T3 | Distinct concepts have distinct types and one simple entry door. |
| N1 | Names follow one subject-first grammar, including acronyms. |
| N2 | Pure operations use noun/past-participle names; mutating operations use imperative names. |
| L-A | Each I/O domain has a whole-value call over a streaming seam. |
| L-B | Concrete containers are eager; `.lazy` opts into the same deferred vocabulary. |
| L-C | New containers implement the one iteration protocol and inherit its adapters. |
| L-D | Beginner presets compose the small expert primitives; they do not fork them. |
| E1 | A retired spelling is deleted in the same greenfield cutover. |
| E2 | A measured gap-filler is absorbed with the wrapper's useful defaults. |

### Part A relation map

This is the proposal's mapping from the existing laws to the Part A extension.
It records which rule is extended and which rule is new ground.

| Existing law | Part A rule(s) | Effect |
|---|---|---|
| L1 naming | N1, N2 | extends: grammar test, side-effect pairs |
| L2 fallibility | F1, F2 | extends: one-character test, sentinel ban |
| L3 ownership | T2 | unchanged; values immutable |
| L4 effects | — | unchanged |
| L5 allocation | L-A, L-B | extends: whole-value layer, eager default |
| L6 diagnostics | F3 | unchanged, restated |
| L7 examples | — | unchanged |
| L8 one way | D3, L-D, E1 | extends: defaults not overloads, presets not forks |
| new ground | C1–C3, D1, D2, D4, T1, T3, E2 | new laws |

### Worked review failures

These examples show the rejection, the reason, and the reviewable replacement.
They are doctrine examples, not additional API declarations.

#### D4: Boolean policy option

```jet
// Rejected: the call does not say what `true` selects.
parse(text, true)
```

The option is a policy choice, so the call names the choice with a dedicated
enum and a label:

```jet
parse(text, on_error: .Lenient)
```

Returning a Boolean fact, accepting a Boolean data value, or storing a Boolean
inside an enum payload is not this D4 failure. The audit below covers only
user-facing policy and configuration choices.

#### F2: Sentinel lookup result

```jet
// Rejected: the same Int is both an index and the "absent" signal.
find_or_minus_one(items, needle) // returns Int; -1 means absent
```

The lookup result carries absence in its type instead:

```jet
items.find(needle) // illustrative result: Item?
```

The caller handles `None` as absence or propagates it with the ordinary
optional path. It does not compare a valid value with a sentinel or inspect a
second status result.

### D4 options audit

The audit covers user-facing policy and configuration choices in the current
Core declarations and the ratified edition-gated encoding surface. It excludes
Boolean results, predicates, data fields, enum payload data, implementation
parameters, and compiler-only handles. An existing Boolean option is marked
drift; it is not a grandfathered exception.

| Surface | Current option shape | D4 result | Card home |
|---|---|---|---|
| `core.http.client` / `HTTPRedirectPolicy.Follow` | `protocols`, `allow_http_downgrade`, and `same_origin_credentials` are Boolean policy values. | Existing drift; replace with named enum choices in a later reconciled change. | #301, #1725 |
| `core.http.server.static_files` | `index`, `dotfiles`, and `follow_links` are Boolean policy values. | Existing drift; the safe bare mount remains documented. | #1273 |
| `core.http.server.cors_policy` | `credentials` is a Boolean policy value. | Existing drift; the `.Any` safety rejection remains a separate policy fact. | #1273 |
| `core.encoding` | `json.writer` `canonical`, `CBOROptions.require_canonical`, `XMLCanonical.comments`, and edition-gated `allow_*` flags are Boolean policy values. | Existing edition and encoding-surface drift; no new encoding behavior is chosen here. | #712 |
| `core.regex.flags` | Three Boolean flag arguments configure the regex policy. | Existing drift; the regex surface stays on its owner card. | #1471 |
| `core.net.set_nodelay` | A Boolean argument selects socket behavior. | Existing drift; low-level control remains on the typed network surface. | #300 |
| `HTTPProxy`, `HTTPRedirectPolicy`, `HTTPRetryPolicy`, `HTTPCorsOrigins` | Named policy enums with dot-shorthand. | Conforms to D4. | — |

The audit is closed for review only when every current exception has a card
home and the changed surface has no new Boolean or bare-string policy option.
The known-drift table above is the card ledger for the exceptions found in the
same audit.

### Magic defaults and expert overrides

This is the one current defaults table for the Part A worked doors and every
current option-bearing Core surface in the D4 audit. APIs with no magic default
do not need a row. New entries must extend this table or reuse an existing
option.

| Door | Bare default | Explicit control |
|---|---|---|
| `files.read(path)` / `files.write(path, text)` | Whole-value UTF-8 file operation; write is safe for the normal path. | `open`, `create`, `append_all`, or labeled write mode. |
| `http.get(url)` | One-shot HTTPS request with the safe client defaults: bounded redirects, safe stale-connection retry, environment proxy use, and no HTTPS-to-HTTP downgrade. | `http.client` for timeout, redirect, retry, proxy, and transport policy. |
| `http.client` | Follow at most 10 redirects, keep same-origin credentials, use safe retries, use the environment proxy, and deny HTTPS-to-HTTP downgrade; cookies stay opt-in. | `.redirects(.Follow.{ max:, same_origin_credentials: })`, `.retries(.Safe/.Idempotent/.None)`, `.proxy(HTTPProxy)`, `.allow_http_downgrade(Bool)`, and `.cookies(.Memory)`; current Boolean controls remain D4 drift. |
| `http.server.static_files(mux, prefix, root)` | Normalize the root, refuse escapes, hide dot-files, refuse escaping links, and serve `index.html` for a directory request. | `index`, `dotfiles`, and `follow_links`; current Boolean controls remain D4 drift. |
| `http.server.cors_policy(origins)` | No CORS header exists until a policy is installed; the safe constructor rejects an unsafe origin/credential combination. | `methods`, `headers`, `credentials`, and `max_age`; `credentials` remains D4 drift. |
| `encoding.*.reader` / `encoding.*.writer` | `EncodingLimits.safe()` bounds the codec; JSON writing is non-canonical unless requested. | `limits: …` and JSON `canonical: …`; the current Boolean canonical control remains D4 drift. |
| `list.map` / `list.filter` | Eager plain collection. | `.lazy` for a deferred view. |
| `time.now` | Current Unix time in milliseconds from the ambient standard clock. | `time.clock(seed)` or an injected `Clock` for deterministic/reproducible code. |
| `crypto` | Typed safe values and fail-closed defaults. | `crypto.expert` inside the audited raw-byte boundary. |

## Competitive Core API gate (D-STDRUBRIC1=A, card #1398)

This is the one Core API superiority gate. Python is the calibration arm. The
release claim covers every language recorded in the Core surface ledger, not
Python alone.

The only workflow inventory is
[`docs/reference/core-surface-ledger.json`](../reference/core-surface-ledger.json).
The checker reads its `rows` and requires one `coreApiGate.workflowManifest`
entry for every row. It does not copy the inventory into another policy or
benchmark document.

### Frozen task record

Before comparison, each workflow records these fields:

| Field | Required content |
|---|---|
| `task` | Stable task identity and ledger-row link. |
| `input` | Same input for every language arm, with the fixture status frozen before the run. |
| `outcome` | Same semantic result, exit behavior, and normal language contract. |
| `allowedDependency` | Standard-library or shipped-Core boundary. Required imports count. |
| `toolVersions` | Pinned competitor and runner versions. |
| `sourceBoundary` | User-authored source paths. Generated output, expected output, and reference source are excluded. |
| `competingCoreWorkflow` | The exact workflow named by the ledger row. |
| `cases` | Applicable `beginner`, `expert-policy`, `failure`, and `lifecycle` arms. |

A design decline stays in the manifest as a scored loss. Only a ratified
product-scope decision can set `scope.excluded` to true.

### Score record

Each matched task reports:

- raw source counts for every arm, including imports, required policy, and
  required error handling;
- mandatory concept IDs, hidden facts, and nonlocal lookups;
- every extra Jet construct, classified as `task-essential`, `clarity-bearing`,
  `guarantee-bearing`, `expert-control`, or `incidental-ceremony`;
- the extra construct's span and source cost, one or more claimed
  `claimedClarity`, `reasoningBenefit`, `localFactBenefit`, `guaranteeBenefit`,
  or `expertControlBenefit` fields, the rejected shorter form, lost value, and
  reviewer verdict;
- the measured reasoning burden; a worse burden needs a compensating product
  win in the same evidence record;
- at least one measured or independently reviewed Jet win;
- independent acceptance that each competing fixture is idiomatic and minimal
  for the same task, input, outcome, and normal language contract.

Raw counts are evidence, not a universal ratio. An increase passes only when it
improves clarity, local reasoning, a named guarantee, or expert control.
Incidental ceremony fails. A worse reasoning burden needs a compensating
product win. Python does not imitate a Jet-only guarantee.

Readability and reasonability use the structured evidence record plus an
independent review. Runtime, memory, artifact, safety, diagnosis, bounds, and
audit properties use machine measurements. The gate fails on stale fixtures,
unexplained ceremony, missing evidence, a missing Jet win, or an unowned loss.
Every failure names card `#1398` as the release-gate owner.

The gate reuses the existing agent corpus manifest, receipt, runner, and
`#769` scoring contract. It adds no benchmark runner and no second scoring
model:

~~~sh
node scripts/agent/check-core-surface-ledger.mjs --check
node scripts/agent/check-core-surface-ledger.mjs --core-api-release-check
~~~

`--check` proves the source-derived inventory and frozen record shape. The
release check stays blocked until every manifest entry has complete measured
evidence and an accepted Jet win.
