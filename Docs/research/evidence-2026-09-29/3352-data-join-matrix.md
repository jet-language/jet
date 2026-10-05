# #3352 — Relational jobs on core.data (join matrix)

Date: 2026-09-29. Binary: `jet-debug-snapshot14`.
Findings: `Docs/research/mine-for-jet-2026-09-12.md#finding-iter-f040` (+ iter-f036 via #3348).

## Question

Which LINQ/itertools relational jobs does the existing core.data surface cover?
The jobs are: inner, left, right by operand reversal, group join, sorted merge
join, and grouped aggregation over List and one-shot Iter input. For each, what
cardinality, projection, missing-side, order and DataLimits contract applies?

## Method

- Source read: `Core/data/data.jet:154-160` (`DataLimits`), `:278-281` (`DataJoin<L,R>`), `:735-761` (`inner_join`), `:763-795` (`left_join`), `:502-508` (`DataLimits.safe()`: `max_join_rows` 1_000_000).
- Witness: `~/.cache/jet-dev/scratch/Closer01/parked/data_join_matrix.jet` (parked, not blessed). It starts from the #3352 overnight draft and uses explicit `inner_join<Order, Owner>` type arguments.
- Ran `safe-jet.sh run`, `run --interpret` and `build`. Scratch bisection files are `dj_nolimit.jet`, `dj_noprint.jet`, `lazy_filter.jet`, `lazy_filter2.jet`, `lazy_cap.jet` and `lazy_cap2.jet` under `~/.cache/jet-dev/scratch/Closer01/`.

## Evidence (observed, `jet run`)

```
inner rows: 5
inner: order 1 core -> Ada
inner: order 1 core -> Lin
inner: order 2 tools -> Grace
inner: order 3 core -> Ada
inner: order 3 core -> Lin
left rows: 6
left: order 1 -> Ada        (… 3 -> Lin)
left: order 4 -> <none>
reversed rows: 6
reversed: Ada -> order 1
reversed: Ada -> order 3
reversed: Lin -> order 1
reversed: Lin -> order 3
reversed: Grace -> order 2
reversed: Kay -> order <none>
group join: order 1 -> [Ada, Lin]
group join: order 2 -> [Grace]
group join: order 3 -> [Ada, Lin]
group join: order 4 -> []
count_by keys: [core, docs, tools]
count_by values: [2, 1, 3]
iter count_by keys: [core, docs, tools] values: [2, 1, 3]
one-shot buffered rows: 3 join rows: 5
limit: inner_join: max_join_rows exceeded
```

Other tiers:
- `--interpret` (the program without the limit block, `dj_nolimit.jet`): identical except `one-shot buffered rows: 0 join rows: 0`. In the interpreter every `xs.lazy()…to_list()` returns `[]` (`lazy_filter2.jet`: `lazy gt: []`, `lazy map: []`, where `jet run` gives `[2, 3]` and `[2, 4, 6]`). With the 1001×1000 limit block included, the interpreter did not finish within 300 s.
- `jet build`: the generated Rust fails with `E0308 expected JetInt, found i64` on a `.max_join_rows` field read. It fails both with the program's own `print(data.DataLimits.safe().max_join_rows)` (`data_join_matrix.rs:171967`) and **without** it (`closer01_djm_probe.rs:171847`). So the read inside `Core/data/data.jet` `inner_join`/`left_join` (`if out.len() >= limits.max_join_rows`) breaks AOT for every join. No AOT output could be observed.

## Contract matrix (criteria 1-3)

| job | Jet program | cardinality | projection | missing side | order | limits / buffering |
|---|---|---|---|---|---|---|
| inner join | `data.inner_join<L,R>(l, r, lk, rk)` | product per key (2×2+1 = 5, observed) | `DataJoin{left: L, right: R}` | dropped | left-major, then right encounter order (observed) | eager; both inputs are owned lists; `max_join_rows` 1_000_000 (fixed `DataLimits.safe()`; no caller override) → `DataError{operation: "inner_join", reason: "max_join_rows exceeded"}` (observed) |
| left join | `data.left_join<L,R>` | product per key, plus 1 row per unmatched left | `DataJoin{left: L, right: R?}` | `right = None` (observed) | left-major | same limit |
| right join | `data.left_join<R,L>(r, l, rk, lk)` (operand reversal) | same as left join with roles swapped | **field layout swaps**: the preserved side is `.left`, the optional side is `.right` | `.right = None` (Kay) | **follows the preserved (right) list**, then its matches (Ada→1,3; Lin→1,3) — not the order a SQL RIGHT JOIN over (orders, owners) would list | same limit |
| group join | `r.group_by(rk)` then `by_key.get(lk(x)) ?? []` per left row | exactly one output per left row | `(L, [R])` shape built by the caller | empty list | left order; each group keeps right encounter order | eager `[K:[R]]`; not charged against DataLimits |
| grouped aggregate | `xs.count_by(k)`, `xs.lazy().count_by(k)`, `data.query(xs).group_by(k).count/sum/mean()` | one per key | `[K:V]` | n/a | **key-sorted**, not first-seen (input order tools, core, docs → `[core, docs, tools]`) | List and Iter agree on `jet run`; Query groups are charged against `max_groups` (existing golden data_hostile) |
| one-shot source | `iter.to_list()` then join | the Iter is drained once into an owned list | — | — | source order | **explicit buffering**: joins take `[T]`, so an Iter cannot be joined without collecting it first |
| sorted merge join (`merge_join_by`) | **no operator** | — | — | — | — | uncovered |

Criterion 2: the reversed left join is **not** equivalent to a right join. The
field layout swaps (`row.left` is the owner) and the encounter order follows the
owners list.

Criterion 3: joins buffer both inputs as lists and fail through the `DataError`
Limit. Group join buffers the right side into a map and has no limit. A sorted
merge join would stream both sides without buffering. Nothing in Core provides
one, and one-shot Iter input must be collected by the caller.

## Uncovered jobs → own ballots (criterion 4)

Drafts for Pip. There is no parallel IterJoin carrier, and I wrote no code.

- **Ballot draft: `data.group_join`.** Option A: add `group_join<L,R>(l, r, lk, rk) -> [DataGroupJoin<L,R>] DataError!` with `{left: L, rights: [R]}`, charged against `max_join_rows`. Option B: document the `group_by` + `get` idiom shown above. Recommendation: B. The idiom is two lines and keeps the lookup visible.
- **Ballot draft: sorted merge join over Iter.** Option A: add `data.merge_join<L,R>(l: Iter<L>, r: Iter<R>, lk, rk)` that requires ascending keys, streams both sides without buffering, and fails with `DataError{kind: InvalidArgument}` on an out-of-order key. Option B: decline and keep eager joins. Recommendation: A only with a streaming workload plus a paired performance cell (AGENTS.md performance gate).
- **Ballot draft: caller-supplied join limits.** `inner_join`/`left_join` always charge `DataLimits.safe()`. Option A: an optional `limits: DataLimits{DataLimits.safe()}` parameter. Option B: keep fixed limits.

## Verdict

- Criteria 1, 2, 3 and 4 are met by this document (matrix, reversal accounting, buffering contracts, ballot drafts).
- Criterion 5 is **not met**: the interpreter's lazy `to_list` is wrong, AOT is unobserved or failing, and the golden is parked.
