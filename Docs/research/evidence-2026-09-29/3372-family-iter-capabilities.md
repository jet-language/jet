# #3372: Per-family iterator capability map

Date: 2026-09-29. Binary: jet-debug-snapshot14, run through `~/.cache/jet-luna/safe-jet.sh`.

## Question

Which order, exact-size and reverse facts does each collection family really
have? Can any iteration view mutate a key or a set element?

## Sources read

- `Compiler/JetFoundation/Source/Collections.jet:250-354`: the operation rows
  for Set, Rank, Queue and PriorityQueue.
  - PriorityQueue has no raw iteration surface. It has only `peek`, `pop`,
    `to_sorted_list`, `remove`, `push`, `len` and `clear`.
  - Rank has no range operation.
  - Set `values` is an `ElementIter` (Adapt law).
- `crates/jet-codegen/src/Prelude/PortableAlloc.rs:660`:
  `pub type JetMap<K, V> = BTreeMap<K, V>`. Map is a key-ordered map, not a
  hash map and not insertion-ordered.
- `Docs/spec/reference/core-library.md:279-282`: Map "Ordered keys"; Set is
  a "Hash-set".

## Evidence

Example: `Examples/features/collections/family_iter_capabilities.jet`. The
`jet run` (JIT) output is in
`Examples/features/expected/collections/family_iter_capabilities.out`:

```
set sorted: [10, 20, 30]
set len: 3
map keys in key order: [apple, fig, pear]
map values in key order: [1, 2, 3]
rank order: [10, 15, 20, 30]
rank first/last: 10 30
pq peek: 9
pq sorted list: [9, 7, 4, 1]
pq pop: 9 then 7
pq len after pops: 2
queue front/back: 1 3
queue reversed: [3, 2, 1]
keys len: 3
filtered keys len: 2
union sorted: [10, 20, 30, 40]
union len: 4
```

`~/.cache/jet-test-scratch/Closer08/maporder.jet` shows the same Map order on
JIT and the interpreter. Literal `{"pear", "apple", "fig"}` lists as
`[apple, fig, pear]`. A later `add("banana")` lists as
`[apple, banana, fig, pear]`.

UI probe: `tests/ui/iter_family_mutable_key.jet`, run with `safe-jet.sh check`.

- `loop k in m.keys() { k = "z" }` is rejected with **E0111**: "`k` was made
  with `::`, so it can't change".
- `loop k in &m.keys()` is rejected with **E0225**: "This call only reads its
  receiver, so it takes no `&` mark".
- `loop n in s.values() { n += 10 }` is rejected with **E0111**.

No path to a writable key or set element was found. `Set.each` accepts no
`&T` parameter (`(n: &Int)` is a parse error, E0003). `callback_mutable` in
`EachMut` only means that the closure captures mutable state.

## Capability table (observed + source)

| Family | Iteration order | Exact size | Reverse | Key or element writable |
| --- | --- | --- | --- | --- |
| List `[T]` | insertion/positional | `len()` | `sort_desc`, `reverse` on List | elements yes (positional) |
| Map `[K:V]` | ascending key order (BTreeMap) | `len()`; `keys()`/`values()` are plain Iter | none on the iterator | keys no (E0111/E0225); values via `m[k]` |
| Set | unspecified (hash). The golden prints it only after sorting | `len()` | none | no (E0111) |
| Rank | comparator (ascending) order; `first`/`last` follow it | `len()` | none on the iterator | no mutation view exists |
| PriorityQueue | no raw iteration; `pop`/`peek` highest first; `to_sorted_list` | `len()` | n/a | no view exists |
| Queue | front-to-back; `peek_front`/`peek_back` | `len()` | `reverse()` mutates the Queue | not tested |
| Set algebra (`union`, ...) | a fresh Set, unspecified order | `len()` of the result | none inherited | n/a |
| `keys().filter(...)` | follows the source | unknown until counted (`to_list().len()`) | none | n/a |

Unknown cells that were kept:

- Rank range cardinality: there is no range operation.
- Queue element mutation through iteration: not probed.

## Verdict

- Criteria 1-4 hold on the source contract and on the JIT run.
  - Hash Set order is never promised; the golden sorts it first.
  - Map order comes from the key comparator.
  - PriorityQueue promises no sorted raw iteration; it has no raw iteration.
  - Set algebra returns a fresh Set with no inherited order.
  - Keys and elements cannot be written through views.
- Criterion 5 is unmet. The interpreter and AOT cannot run the example (see
  Defects).

## Defects and findings

1. Interpreter: the Set surface is not supported (E0956). This covers
   `set_sort`, `set_values` and `set_to_list`.
2. AOT: every Queue use crashes with `MIR nominal type "Queue" has no
   declaration row` (MIRRust.rs:2699). The repo example
   `Examples/features/collections/queue.jet` fails the same way.
3. AOT: `Rank.last()` produces generated Rust that does not compile: it calls
   `jet_sorted_set_last`, and no such Prelude fn exists.
4. AOT: `{&pq.pop() ?? -1}` inside string interpolation passes
   `&BinaryHeap` to `jet_priority_queue_pop_kernel`, which expects `&mut`,
   so rustc fails.
5. Doc defect: `Examples/features/collections/collection_contracts.jet:28`
   says Map is "ordered by first insertion; ... re-add after remove moves to
   the end". The runtime is a BTreeMap, which lists keys in key order. That
   example has no golden to catch this.
6. Diagnostic quality: for a loop binding, E0111's fix says "Declare it with
   `k := ...`". A loop binding over map keys cannot be declared that way, and
   even a mutable copy would not rename the key.
