# #3281 — Collection families: shipped types and their contracts

Date: 2026-09-29. Binary: `jet-debug-snapshot14` (via `~/.cache/jet-dev/safe-jet.sh`).
Finding: `Docs/research/mine-for-jet-2026-09-12.md#finding-core-f060`.

## Question

Which Jet types serve the sequence, map, set, queue, heap, ordered, immutable
and concurrent collection families? What ordering, equality/hash,
mutation/invalidation, snapshot/borrow and allocation contract does each one
have? Which useful families are missing?

## Method

- Source read: `Compiler/JetFoundation/Source/Collections.jet:13-27`
  (`SemaCollectionKind`) and the `@COLLECTION_OPERATIONS` rows (Tally rows at
  :312-319, Cache rows at :320-322); `Docs/spec/reference/core-library.md:254-304`;
  `Core/collections/collections.jet:14-49`; `Core/collections/set.jet`.
- Witness program (one block per family), parked because the tiers disagree:
  `~/.cache/jet-dev/scratch/Closer01/parked/collection_contracts.jet`.
  I ran it with `~/.cache/jet-dev/scratch/Closer01/tiers.sh collections/collection_contracts`
  (`jet run`, `jet run --interpret`, `jet build` + binary). The observed `jet run`
  output is in `.../parked/collection_contracts.run.observed`.
- Scratch probe: `~/.cache/jet-dev/scratch/Closer01/tally.jet`.

## Evidence (observed on `jet run`)

```
list order: [3, 1, 2, 0]
list equal is positional: true false
list copy independent: [3, 1, 2]
map order: [a, b, c] b=10
map order after re-add: [a, b, c]
map equal ignores insertion order: true
set len after dedupe: 3
set equal by membership: true
set sorted view: [1, 2, 3]
rank order: [0, 1, 2, 3]
rank first/last: 0 3
queue order: [0, 1, 2, 3]
queue capacity covers len: true
queue pop_front: 0
priority order: [7, 5, 4, 3, 1]
priority pop: 7
cache get a: 1
cache keys after eviction: [c, a]
cache has b: false
tally x: 0 y: 0 z: 0          <- wrong, see defects
bits order: [2, 9] copy: [2, 5, 9]
view writes: 413 476 read: Emma
lazy to_list: [2, 4, 4]
to_set len: 2
```

Other tiers:
- `jet run --interpret` stops at the first Set equality: `E0956 core.builtin.set_equal() isn't supported by the current evaluator yet` (line 42).
- `jet build` ICEs: `MIR nominal type "Queue" has no declaration row` (crates/jet-codegen/src/Codegen/MIRRust.rs:2699).
- `tally.jet`: `Tally<Int>` counts correctly (`count 4 2`). `Tally<String>` loses its elements (`has x false count x 0`). `Tally.len()` ICEs (`builtin method BagLen ... requires structural MIR lowering`). The interpreter rejects `Tally.new` (`E0956 std::collections::HashMap.new()`). `Tally.from(xs)`, documented at core-library.md:285, fails with `E0107 Nothing named Tally`.

## Contract table (criterion 1)

The Order column comes from observation. The equality/hash, invalidation,
borrow and allocation columns come from source and docs; I did not measure them.

| Family | Jet type | Order | Equality / hash | Mutation & invalidation | Snapshot / borrow | Allocation |
|---|---|---|---|---|---|---|
| sequence | `[T]`, `FixedList` | insertion (observed) | positional structural `==`/`equal` (observed) | `&xs.push(…)` etc. need a write place; mutation during traversal is a compile error (`tests/ui/list_loop_mutate.jet`) | `copy()` is an independent snapshot (observed); `View<T>`/`ViewMut<T>` windows `xs[i..j]` borrow with provenance | growable heap buffer; `copy` allocates |
| map (sorted) | `[K:V]` | **key order**, independent of insertion, overwrite or remove/re-add (observed) | `==` ignores construction order (observed); keys restricted by E0502 | overwrite keeps one entry (observed) | `copy()` | heap |
| hash set | `Set<T>` | unspecified hash order; `sort()` returns a fresh list | membership `equal` (observed); elements need `Hash + Eq` (E0506) | `add`/`remove`/`pop` | `copy()`, `values()` lazy | heap |
| ordered set | `Rank<T>` | ascending (observed) | membership | `add`/`remove` | `to_list()` copy | heap |
| deque / queue | `Queue<T>` | FIFO at both ends (observed) | — | `push_*`/`pop_*`/`delete`/`split` | `to_list()` copy | ring buffer; `capacity() >= len()` (observed) |
| heap | `PriorityQueue<T>` | highest first (observed); ties not further ordered | — | `push`/`pop`/`remove(x, .Slot)` | `to_sorted_list()` copy | heap |
| bounded map | `Cache<K,V>` | recency; `keys()` most recent first (observed `[c, a]`) | — | `get` refreshes recency; `add` past capacity evicts the least recent (observed) | — | fixed capacity |
| multiset | `Tally<T>` | — | — | `add`/`remove`/`count` | — | heap; defective for String elements |
| int set | `Bits` | ascending (observed) | — | `add`/`remove` | `copy()` independent (observed) | bitset |
| bytes | `Bytes` | positional | — | cursor writes | `copy` | growable |

Core/collections (`Counter`, `Deque`, `OrderedMap`, `Chain`, `StringSet`, Int
heap helpers `nsmallest`/`nlargest`) are String- or Int-specialised
Python-compatible duplicates of Tally, Queue, the map and Set.
`Core/collections/collections.jet:34` `OrderedMap` is the only
**insertion-ordered** map. The built-in `[K:V]` is key-ordered. Retiring or
generalising these duplicates needs its own card.

## Families versus producers and destinations (criterion 2)

- **Families** (own storage): `[T]`, `FixedList`, `[K:V]`, `Set`, `Rank`, `Queue`, `PriorityQueue`, `Cache`, `Tally`, `Bits`, `Bytes`.
- **Iterator producers** (no storage; one-shot): `.lazy()` → `Iter<T>`, `ViewIter`, String splitting, file lines, streams, channels (core-library.md:256-262).
- **Borrowed windows**: `View<T>`/`ViewMut<T>` (slices of a family; not a family).
- **Collect destinations**: `to_list()`, `to_set()`, `collect()`, `count_by`/`group_by` (→ `[K:V]`), `to_sorted_list()`.

## Missing families (criterion 3): one scope ballot each

These are drafts for Pip to file. Each ballot is independent. Do not build a
generic collection tree.

### Ballot draft A — insertion-ordered map
- Job: iterate a map in insertion order (Python `dict`, Java `LinkedHashMap`, JS `Map`).
- Evidence: `[K:V]` iterates in key order (observed). Only the String-only `core.collections.OrderedMap` keeps insertion order.
- Options: **A** document `[K:V]` as a sorted map and generalise `OrderedMap<K,V>` in core.collections. **B** add an insertion-order builtin kind next to `[K:V]`. **C** no change; applications keep a `[K]` key list next to the map.
- Recommendation: A. It adds no new builtin kind and removes the String-only limit.
- Beginner path: `m := OrderedMap<String,Int>.new()`, then `&m.add(k, v)`. Expert path: the same type, plus `keys()`/`values()` views.

### Ballot draft B — immutable / persistent collections
- Job: cheap structural-sharing snapshots (Clojure vectors, Scala `Vector`, `im`).
- Evidence: every family snapshots by `copy()`, which is a full copy (source: `CollectionOp.Copy` rows).
- Options: **A** a persistent `PVec<T>`/`PMap<K,V>` pair in core.collections. **B** no family; document `copy()` plus value semantics as the contract.
- Recommendation: B, until a measured workload shows copy cost. The performance gate would require a paired cell.

### Ballot draft C — concurrent map / queue
- Job: shared mutable map or queue across tasks (Java `ConcurrentHashMap`, Go `sync.Map`).
- Evidence: channels are the only concurrent queue. `core.sync` `SyncMap` is a CRDT merge type, not a shared-memory map.
- Options: **A** a `SharedMap<K,V>` handle under core.sync with lock-free reads. **B** no family; channels plus task-owned maps are the one pattern.
- Recommendation: B. It keeps one ownership model. Revisit if a concurrent workload needs it.

Sorted map and ordered set need no ballot: `[K:V]` (key-ordered) and `Rank<T>` already ship.

## Verdict

- Criteria 1, 2 and 3 are met by this document.
- Criterion 4 is **not met**. The witness is parked and I did not bless it, because the tiers disagree (interpreter E0956 on `Set.equal`, AOT ICE on `Queue`) and `Tally<String>` is wrong on `jet run`.
