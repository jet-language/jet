# Core API ergonomic laws (D-STDRUBRIC1=A)

Review rubric for all Core API additions. Every new function, method, or type
must pass each law before landing. No exceptions; file a follow-up card for any
existing drift found during review.

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

- A function that can legitimately fail returns `T ? E`; never panics on expected failure.
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

- I/O effects are declared with the right effect marker (`=[FS]=>`, `=[Net]=>`,
  `=[Exec]=>`, etc.).
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

- Every fallible function that returns `? E` has at least one corresponding UI snapshot
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
  without adding new capability (e.g. `slice.first()` over `slice[0]?`).

### Print family (D-ONCE-PRINT1=A)

The family has one job per spelling. Beginners learn ambient `print` first.

| Spelling | Job | Status and default |
| --- | --- | --- |
| `print(value)` | Display a value and end the line. | Beginner default; no import. |
| `term.print(value)` | The same line-ending print through `core.term`. | Qualified twin for `#NoPrelude` files. |
| `term.println(value)` | No distinct job from `term.print`. | Retired; `jet fmt` and `jet fix` rewrite it to `term.print`. |
| `term.sprint(value)` | No distinct job from interpolation. | Retired; `jet fmt` and `jet fix` rewrite it to `"{value}"`. |
| `term.repr(value)` | No distinct job from debug interpolation. | Retired; `jet fmt` and `jet fix` rewrite it to `"{value:Debug}"`. |

Interpolation is the string-building mechanism. `:Debug` selects the existing
debug representation selector; it is not a second print API.

---

## Review template

When submitting a new Core API for merge, include this checklist in the PR:

```
## Core API review

Function/type: `<name>`
Ratified decision(s): <D-XXX / S-YYY>

- [ ] L1 Naming: plain English, predicate prefixed, S66 acronyms
- [ ] L2 Fallibility: correct return type, panic only on programmer error
- [ ] L3 Ownership: view vs ownership vs mutation explicit
- [ ] L4 Effects: all capability markers declared
- [ ] L5 Allocation: budget documented if non-obvious, streaming form provided
- [ ] L6 Diagnostics: UI snapshot for error paths
- [ ] L7 Examples: golden-tested example in examples/features/
- [ ] L8 One way: no duplicate of existing API
```

---

## Known drift (follow-up cards)

Items below existed before this rubric and have acknowledged gaps. Each has a
Tower card tracking the fix; this list is the authoritative inventory.

| Gap | API | Law | Follow-up |
|-----|-----|-----|-----------|
| — | None recorded after the Core namespace cutover | — | — |

*Resolved gaps leave this table empty; new drift gets a ratified owner card before
it is listed here.*

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
| C2 | Required values are positional; labels make uncommon or ambiguous options readable. |
| C3 | The common dataflow reads left to right through methods and `?`. |
| D1 | The bare call performs the safest common operation with no setup ceremony. |
| D2 | Every magic default appears in the defaults table and has an explicit override. |
| D3 | Defaulted options replace overload families. |
| D4 | Options use dedicated enums; Core does not use boolean or bare-string policy flags. |
| F1 | Expected failure is `T ? E`; propagation costs `?`. |
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

### Magic defaults and expert overrides

This is the one current defaults table for the common Core doors. New entries
must extend this table or reuse an existing option.

| Door | Bare default | Explicit control |
|---|---|---|
| `files.read(path)` / `files.write(path, text)` | Whole-value UTF-8 file operation; write is safe for the normal path. | `open`, `create`, `append_all`, or labeled write mode. |
| `http.get(url)` | One-shot request with the safe client defaults. | `http.client` for timeout, redirect, retry, and transport policy. |
| `list.map` / `list.filter` | Eager plain collection. | `.lazy` for a deferred view. |
| `time.now` | Local current instant with the standard clock door. | `time.now_utc` or an injected `Clock` for explicit zone/reproducibility. |
| `crypto` | Typed safe values and fail-closed defaults. | `crypto.expert` inside the audited raw-byte boundary. |
