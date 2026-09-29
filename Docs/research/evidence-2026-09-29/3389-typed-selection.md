# #3389 — Typed selection without unchecked runtime casts

Closer09, 2026-09-29. Binary: `~/.cache/jet-luna/safe-jet.sh` (jet-current
snapshot14 → snapshot16 during the session). Scratch:
`~/.cache/jet-test-scratch/Closer09/free/`.

## Question

Do tagged-case patterns plus `filter_map` over fallible callbacks cover the
LINQ `OfType`/`Cast` and dynamic-filter jobs with explicit keep/drop/propagate
behaviour, does a matched payload keep its ownership and loan, and is a checked
foreign downcast present or absent?

## Method

`typed_selection.jet` (the planned `Examples/features/patterns/typed_selection.jet`)
over `enum Shape { Circle(Float) Rect(w: Float, h: Float) Label(String) }`:
keep (total pattern table), drop (`loop` + `if s == { .Circle(r) -> … else ->
… }`), drop through `filter_map` (`circle_or_err` returns `Float Err!`),
propagate (`all_circles` returns `[Float] Err!`; the unmarked fallible call
propagates the first rejection), loan (`label_lengths(shapes: [Shape])` reads
payloads of a borrowed list; the caller's list is still usable), ownership
(`take_labels(shapes: ^[Shape])` moves each `.Label(text)` payload out with
`^text`). Negative fixtures: `typed_selection_payload_escape.jet`
(`label_view(s: Shape, spare: String) -> View<str> from spare` returns a view of
the matched payload) and `payload_loan.jet` (same, but declared `from s |
fallback`, which should be accepted).

## Evidence

`run3.sh typed_selection.jet run,interp,aot`:

```
keep circle r=1.5
keep rect 2.0x3.0
keep label   north
keep circle r=0.5
drop Rect
drop Label
loop radii: [1.5, 0.5]
filter_map radii: [1.5, 0.5]
propagated: not a circle: Rect
all circles: [2.0, 3.0]
borrowed label lengths: [9]
list still owned by caller: 4 shapes
taken labels: [east, west]
```

- default `jet run`: exit 0, output above.
- AOT (`jet build`, `.jet/build/typed_selection`): exit 0, identical (`same: aot == run`).
- `jet run --interpret`: exit 1, `E0956: core.list.filter_map() isn't supported
  by the current evaluator yet` at 81:5.

Loan through a matched payload (`jet check` / `jet run`):

- `payload_loan.jet` (`-> View<str> from s | fallback`, returns
  `text.trim()` of `.Label(text)` matched from parameter `s`): **rejected**,
  `E2307: found cannot be returned without an owning copy … found is a
  zero-copy view into text`.
- `typed_selection_payload_escape.jet` (`from spare`, a real escape): rejected
  with the same E2307 text. The diagnostic does not say the view escapes its
  `from` clause (compare the existing `tests/ui/view_from_escape.stderr`,
  E2305 "Returned view escapes its declared `from` clause"); it treats the
  payload binding `text` as its own owner.
- `sel/sel_view2.jet` (loop over `shapes: [Shape]`, return a view of the
  matched `.Label(text)` under `from shapes | fallback`): same E2307.

Checked downcast: no downcast / `try_as` / `as?` construct exists
(Pip's triage grep over Core/, Compiler/JetSema, Examples/features; not
re-run). Foreign values arrive through typed binders (`#Import module`,
`extern rust`) with declared Jet types, so there is no erased value to
downcast; nothing was added.

## Verdict

- Criterion 1 (payload keeps exact ownership and loan): **partial/fail**.
  Ownership is right: a take-mode loop moves payloads (`taken labels: [east,
  west]`) and a borrowed loop leaves the caller's list usable. The loan is not
  kept: a view of a matched payload cannot be returned under a `from` clause
  that names the matched value's owner (E2307), so provenance stops at the
  pattern binding.
- Criterion 2 (explicit keep/drop/propagate): met on default run and AOT (rows
  above). Downcast rows: absent capability, no row to test.
- Criterion 3 (no RTTI/erasure feature): met; nothing introduced.
- Criterion 4 (golden + UI snapshot): not met. The golden would fail the
  interpreter tier (E0956 `filter_map`), and the escape fixture's diagnostic
  is the owner-less E2307 rather than a `from`-escape diagnostic. Nothing
  blessed; files stay in scratch.

## Defects

1. Interpreter: `core.list.filter_map()` unsupported (E0956) while default run
   and AOT run it. Repro: `free/typed_selection.jet`, line 81.
2. Pattern payload provenance: a view of a payload matched from a parameter is
   treated as a view into a function-owned local. Repro `free/payload_loan.jet`:
   expected accepted (view borrows `s`), observed E2307.

## Follow-up

A later `of_case`-style adapter or any foreign downcast helper needs its own
ballot; no evidence here shows one is missing.
