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
- Boolean predicates are verb-prefixed: `is_empty`, `has_prefix`, `contains`.
- Fallible variants add no suffix; the `?` return type signals fallibility.
- Constructor idioms (D-API-CTOR1=A): bare `Type(args)` when the args are the value's
  components; `Type.new(…)` for fresh stateful containers; `Type.over(x)` for non-owning
  views over existing data; `Type.from_*(x)` for conversions. `Type.{ }` stays the
  literal for plain data records (D-DOTCTOR1). New construction shapes need a ballot.
- Standard acronyms stay fully capitalized per S66 (`JSONDecoder`, `HTTPClient`,
  `IOError`, `UTF8Error`). Do not add PascalCase aliases.

## Law 2 — Fallibility

- A function that can legitimately fail returns `T ? E`; never panics on expected failure.
- Panics are reserved for programmer error (index out of bounds on a known-size slice).
- The error type must carry enough context to write a helpful error message without
  inspecting source code (no opaque integer codes).
- Use the most specific error type available; `Error` (the Fallible default) is a
  last resort for heterogeneous error paths.

## Law 3 — Ownership / allocation

- Functions that only read a value take bare `T`; unmarked read access is enforced
  and never elevates.
- Functions that return a new allocation return by value; they do not write into a
  caller-supplied buffer unless the API is explicitly a low-allocation path.
- Mutation is visible: a function that mutates a value takes `&T`; ownership
  transfer takes `^T`.
- `@SingleUse` types must be documented with the invariant they enforce.

## Law 4 — Effects

- I/O effects are declared with the right effect marker (`#(Fs)`, `#(Net)`,
  `#(Exec)`, etc.).
- Pure functions carry no effect markers; the compiler enforces this.
- A function that performs multiple effects lists all of them; no hidden IO.
- Comptime-evaluable functions satisfy D-CTCORE1's pure-Core whitelist.

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
- When a type or method is `@MustUse`, the diagnostic names the missed call.

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
