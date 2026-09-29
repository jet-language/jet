# #3378 — sort-key evaluation, stability and tie policies

Date: 2026-09-29. Binary: jet-debug-snapshot14 via `~/.cache/jet-luna/safe-jet.sh`.

## Question

What does the existing `sort_by` contract guarantee (stability, key-callback
count and order, failure order, descending and secondary keys), and is any
cached-key, unstable or ThenBy spelling justified?

## Sources read

- `Compiler/JetFoundation/Source/Collections.jet:241` CollectionLaw.SortBy:
  eager and in place; "key or comparator calls in the sort's order; count and
  order are unspecified"; "the first callback Err is the result".
- `:404-405,812-817`: `sort_by` takes a key or an `Ordering` comparator and
  selects SortBy / SortByCompare / TrySortBy; `sort_by_desc` takes a key.
- `tests/tir_core_and_closures.rs:637-674`: fallible sort "evaluates each key
  once in source order and leaves the receiver untouched".

## Evidence

`Examples/features/collections/sort_key_contract.jet`; interpreter
(`run --interpret`) and AOT (`build` then `.jet/build/sort_key_contract`)
outputs are byte-identical and form
`Examples/features/expected/collections/sort_key_contract.out`:

- stable: `b:1:9 d:1:3 a:2:5 c:2:1 e:2:5` (equal keys keep input order).
- key calls for `[5,4,3,2,1]`: `key(5) key(4) key(3) key(2) key(1)`, five
  calls in source order: one key per element, computed before sorting.
  Nothing is cached across sorts or shared between elements.
- fallible key: `try-key(50) try-key(40) try-key(30)`, then
  `sort failed: no key for 30`. Later keys are never requested, and the list
  stays `[50, 40, 30, 20, 10]`.
- one comparator closure, descending team and then ascending cost:
  `c:2:1 a:2:5 e:2:5 d:1:3 b:1:9`. a stays before e, so ties are stable.
- `sort_by_desc`: `a:2:5 c:2:1 e:2:5 b:1:9 d:1:3` (stable descending).
- `max_by` gives the LAST tie (`e`), while a stable sort keeps the FIRST tie first
  (`a`). These are separate policies.

`safe-jet.sh run` (JIT) crashes after `key(3)` with
`jit checked try_map: callback returned an invalid Result`.

## Verdict

The existing composition covers every case the finding raises: a stable key
sort, one key call per element (already "cached" per sort), an atomic fallible
sort, and an `Ordering` comparator for secondary and descending keys. No
ThenBy alias, cached-key or unstable variant is needed. A future variant
motivated only by performance must first register a matched
`performance_surface_pairs` cell. None is proposed.

The law row still says the count is "unspecified", while every kernel that
ran does exactly one call per element in source order. Pinning that in the
law row is a possible follow-up. It is not done here.

## Defects (tier parity, see closer report)

1. JIT: `sort_by` with a key closure that calls any user function crashes
   (`jit_key2.jet`).
2. All tiers: a sort key closure that writes a captured `&` local fails on
   every tier. The JIT crashes, the interpreter reports E0956, and AOT fails
   rustc E0502 (`cap_ice.jet`).
3. All tiers: a comparator closure whose body calls a named fn returning
   `Ordering` crashes with "closure method without a checked operation"
   (`cmp_ice.jet`).

Repros are in `~/.cache/jet-test-scratch/Closer08/`.
