# Core API ergonomic laws (D-STDRUBRIC1=A)

Review rubric for all Core API additions. Every new function, method, or type
must pass each law before landing. No exceptions; file a follow-up card for any
existing drift found during review.

---

## Law 1 — Naming

- Names are plain English words, not abbreviations (`remove`, not `rm`).
  Blessed exceptions (closed list, D-API-LEN1=A; ballot to extend): `len`,
  and the module names `fmt`, `args`, `env`, `mem`.
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

| Job | List | Map | Set | Deque | PriorityQueue |
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
| L1 | `core.files.read` / `core.files.write` use short names | 1 | c44-follow-1 |

*When a gap is resolved, remove the row and close the follow-up card.*

Resolved disposition, #1691: `core.crypto.expert` now uses distinct
`x25519_raw` / `hkdf_sha256_raw` names, and
`examples/features/crypto/crypto_migration.jet` covers the audited raw path
while the safe `core.crypto` APIs retain typed defaults.

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
  `core.files`, nests random under math and `fmt` under text, merges env and os
  into sys, and splits `core.io`. It does not add `core.json`.
- **D-CORE-USELIST1=A** — every grouped `use` uses square brackets. `as`
  gives a shorter local name; without `as`, the local name is the last part
  after the final dot. Existing brace item imports move to the same list.
