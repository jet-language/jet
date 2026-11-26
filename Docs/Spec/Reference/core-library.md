# Jet Core library reference

`Core/` and `Compiler/Prelude/` are the source of truth for exact module
signatures, types, and behavior. This reference covers cross-cutting Core
contracts, generated collection/module indexes, and non-obvious invariants.
(Full historical per-module prose lives in git history:
`git show 20be9d938a5:Docs/Spec/Reference/core-library.md`.)

<!-- FEATURE_CLAIMS:BEGIN -->
<!-- FEATURE_CLAIM: claim.core-foundation | Core foundations are reachable Jet software. -->
<!-- FEATURE_CLAIM: claim.core-concurrency | Tasks and events share one runtime. -->
<!-- FEATURE_CLAIM: claim.core-files-data | Files, paths, archives, compression, and DB APIs are production claims. -->
<!-- FEATURE_CLAIM: claim.core-encoding-text | Codecs and text follow published standards. -->
<!-- FEATURE_CLAIM: claim.core-network-http | Network and HTTP claims require live interoperability. -->
<!-- FEATURE_CLAIM: claim.core-security | Security APIs require fail-closed entropy. -->
<!-- FEATURE_CLAIM: claim.core-data-compute | Typed data claims require real documented semantics. -->
<!-- FEATURE_CLAIM: claim.core-ui-web | UI and web share one typed component model. -->
<!-- FEATURE_CLAIM: claim.game-product | Game claims require a playable runtime and editor. -->
<!-- FEATURE_CLAIM: claim.plugin-ffi | Plugins and FFI use one typed interop structure. -->
<!-- FEATURE_CLAIMS:END -->

## Importing modules and conventions

- Import Core modules with `use` aliases (e.g., `use files: core.files` or
  `use { files: core.files, text: core.text }`) and call through the alias.
- `core.*` modules live under `Core/`; Prelude types (`Int`, `Float`, `Bool`,
  `String`, `Option`, `Result`, `[T]`, `[K:V]`, `#Error`) are in scope without
  an import.
- **Pay for what you call:** Checking or building a program loads and compiles
  only the Core declarations reachable from the program; `jet check Core` checks
  all of `Core/` including uncalled functions and Core's own lints.
- **Errors and optional values:** Fallible APIs return typed `#Error` results
  (`T E!`). Use `??` for fallback or error propagation and `?` / `Option` for
  absence; never use sentinel return values (`-1`, empty string) for failure.
- **Byte streams:** Streaming readers/writers implement the shared `Reader` and
  `Writer` contracts (`Core/IO/`) so codecs, compression, TLS, and files compose
  without buffering whole payloads into memory.
- **Time zones, DST, format patterns, and leap seconds (`core.time`):**
  - `time.zone(name)` resolves IANA zones from embedded TZif data,
    `JET_TZDB_DIR`, `TZDIR`, or system zoneinfo roots; a missing zone database,
    unknown zone, or invalid traversal name returns a typed `TimeError` and
    never crashes.
  - `time.zoned_local(date, time, zone, disambiguation)` resolves DST gaps and
    overlaps deterministically: `"compatible"` (default) shifts a gap forward
    across the transition (`"later"`) and picks the first occurrence of an
    overlap (`"earlier"`); `"earlier"` and `"later"` select the pre- or
    post-transition offset explicitly; `"reject"` returns a typed `TimeError`.
  - Formatting (`time.strftime`, `.format`, `.format_checked`) and pattern
    parsing (`.parse_format`) share one single-pass `%`-code pattern table
    (`%Y`/`%-Y`, `%m`, `%d`, `%H`, `%M`, `%S`, `%L`, `%f`, `%N`, `%z`, `%:z`,
    `%Z`, `%%`, composites `%F`/`%T`/`%R`/`%D`/`%c`). Literal text and emitted
    zone names are never re-scanned.
  - UTC leap seconds (`23:59:60`) are accepted only on valid IERS UTC leap
    dates, preserve `second() == 60` in formatting, and order strictly between
    `23:59:59` and the following `00:00:00` (`Examples/Systems/Time/{FormatPatterns,LeapSeconds}.jet`).
- **Measurement uncertainty:** The measurement operations in `core.units` use
  **first-order linear propagation with uncorrelated inputs**.
  **Correlated errors are out of scope**.
- **One semantic home:** Every Core API executes the same compiled Jet runtime
  implementation across O0, O1, O2, comptime, and web (I9).
- **Splittable randomness:** In `core.math.random`, a caller-owned `RNG`
  (`random.rng(seed)`) supports pure, order-independent keyed splitting via
  `rng.split(key…)` and direct `random.rng(seed, key…)`. Child streams depend
  solely on the parent's root seed and length-prefixed, domain-separated keys
  (never mutating `self.state` or depending on call order, prior draws, task
  scheduling, or tier), and nested splits are associative (`r.split("a", "b")`
  equals `r.split("a").split("b")`). Zero-argument `rng.split()` retains the
  legacy stateful step, and `core.crypto.random` remains a separate OS CSPRNG.
  See [`Examples/Math/SplittableRandom.jet`](../../../Examples/Math/SplittableRandom.jet).
- **Task-scoped working directory and environment (`#Context(dir:, env:)`):**
  `context.dir` (`String`) and `context.env` (`[String:String]`) carry the
  active task's working directory and environment. `#Context(dir: ..., env: {CC: "clang"}) { ... }`
  scopes both lexically (restoring on exit and inherited by spawned tasks);
  relative paths in `core.files`, `core.db`, and `core.process`, environment
  reads in `core.sys` and `core.process`, and spawned child processes resolve
  against the task context. Imperative mutators (`core.sys.set_current_dir`,
  `core.sys.set`, `core.sys.unset`)
  update only the current task's context frame — no Core API reads or mutates
  a shared process-wide working directory or environment outside the task
  context, so parallel tests and tasks setting different directories never
  interfere.

## Collection operations

Generated from `Core/` by `Tools/Agent/CoreSourceRows.jet` (never hand-edited):

<!-- BEGIN GENERATED COLLECTION OPERATIONS -->
<!-- Source: Compiler/JetFoundation/Source/Collections.jet; regenerate with jet Tools/Agent/CoreSourceRows.jet -->

| Law | Timing | Result | Callbacks | Stop item | Failure channel | Buffering |
| --- | --- | --- | --- | --- | --- | --- |
| Measure | eager | a fresh scalar | none | an iterator receiver is consumed whole | none | none |
| Lookup | eager | an optional copy of the found item or index | none | an iterator receiver is consumed and cannot be resumed | absence is None | none |
| Mutate | eager, in place | Unit, a Bool, or the displaced item | none | not applicable | absence is None; no error channel | the receiver's own storage |
| TryMutate | eager, in place | a Result | none | not applicable | AllocError when storage cannot grow | the receiver's own storage |
| Build | eager | a fresh owned collection | none | the whole receiver is read | none | the whole output |
| Copy | eager | an independent value; storage is shared until the first write | none | not applicable | none | none until the first write |
| Adapt | lazy | an iterator that owns the adapted source | none | pulls only the items it yields or skips | none | none |
| Window | lazy | an iterator of lists | none | pulls one window or chunk at a time | none | one window or chunk |
| Disjoint | eager proof, borrowed windows | tracked exclusive ViewMut slots; edit_disjoint returns Unit | edit_disjoint invokes a callback with two lent views; the other methods have none | no callback runs for an invalid selection | String error for invalid bounds or overlapping selections | view descriptors only; the owner's elements are never copied |
| Visit | eager on lists and maps, lazy on iterators | a List or Map on eager receivers, an iterator on iterator receivers | once per item, in source order | every item is visited | a callback Err is the whole result; no partial output | eager receivers buffer the output; iterators buffer nothing |
| Each | eager | Unit | once per item, in source order | every item is visited | none | none |
| Decide | eager | a Bool, Option or position | once per item, in source order, until the answer is known | the deciding item is consumed; later items are never read and an iterator receiver is dropped | none | none |
| Stream | lazy | an iterator | once per pulled item, in source order | the item that ends take_while is consumed and not yielded | none | the current item or group |
| Fold | eager | the accumulator or chosen item; the seed or None when empty | once per item, in source order | every item is read; equal keys keep the last item | none | none |
| Group | eager | a fresh map, tuple or lists | once per item, in source order, when a key callback is given | every item is read | none | every item is buffered into the output |
| Drain | eager | a fresh value | none | the whole source is consumed | none | the output |
| Reorder | eager: the whole source is read before the first result | the reordered or trimmed items | none | the whole source is consumed | none | the whole source |
| SortBy | eager, in place | Unit, or a Result when the key callback can fail | key or comparator calls in the sort's order; count and order are unspecified | not applicable | the first callback Err is the result | the list and its keys |
| Search | eager | an optional index | once per probed item, in binary-search order | stops at the match | absence is None | none |
| MergeEntries | eager | a fresh map | once per key present in both maps | every entry is read | none | the output map |
| Pull | one item per call | an Option of the next item | none | exactly the returned item is consumed; the remainder stays in the source | end is None | none |

| Receiver | Operation | Receiver ownership | Law | Empty result | Status |
| --- | --- | --- | --- | --- | --- |
| `Set<T>` | `map(fn(item) { T })` | borrows | Visit | empty list | current |
| `Set<T>` | `filter(fn(item) { Bool })` | borrows | Visit | empty list | current |
| `Set<T>` | `each(fn(item) { T })` | borrows | Each | Unit | current |
| `Set<T>` | `all(fn(item) { Bool })` | borrows | Decide | true | current |
| `Set<T>` | `flat_map(fn(item) { T })` | borrows | Visit | empty iterator | current |
| `Set<T>` | `fold(seed, fn(acc, item) { acc })` | borrows | Fold | the seed | current |
| `Set<T>` | `min()` | borrows | Lookup | None | current |
| `Set<T>` | `max()` | borrows | Lookup | None | current |
| `Set<T>` | `clear()` | exclusive, in place | Mutate | Unit | current |
| `Set<T>` | `bits()` | borrows | Measure | 0 | current |
| `Set<T>` | `to_set()` | borrows | Copy | empty Set | current |
| `Set<T>` | `add(item)` | exclusive, in place | Mutate | true | current |
| `Set<T>` | `remove(item)` | exclusive, in place | Mutate | Unit | current |
| `Set<T>` | `discard(item)` | exclusive, in place | Mutate | Unit | current |
| `Set<T>` | `has(item)` | borrows | Measure | false | current |
| `Set<T>` | `pop(item)` | exclusive, in place | Mutate | None | current |
| `Set<T>` | `take(item)` | exclusive, in place | Mutate | None | current |
| `Set<T>` | `replace(item)` | exclusive, in place | Mutate | None | current |
| `Set<T>` | `union(same kind)` | borrows | Build | the argument | current |
| `Set<T>` | `intersection(same kind)` | borrows | Build | empty Set | current |
| `Set<T>` | `difference(same kind)` | borrows | Build | empty Set | current |
| `Set<T>` | `symmetric_difference(same kind)` | borrows | Build | the argument | current |
| `Set<T>` | `equal(same kind)` | borrows | Measure | true when both are empty | current |
| `Set<T>` | `is_subset(same kind)` | borrows | Measure | true | current |
| `Set<T>` | `issubset(same kind)` | borrows | Measure | true | current |
| `Set<T>` | `is_superset(same kind)` | borrows | Measure | true only for an empty argument | current |
| `Set<T>` | `issuperset(same kind)` | borrows | Measure | true only for an empty argument | current |
| `Set<T>` | `is_disjoint(same kind)` | borrows | Measure | true | current |
| `Set<T>` | `isdisjoint(same kind)` | borrows | Measure | true | current |
| `Set<T>` | `update(same kind)` | exclusive, in place | Mutate | Unit | current |
| `Set<T>` | `difference_update(same kind)` | exclusive, in place | Mutate | Unit | current |
| `Set<T>` | `intersection_update(same kind)` | exclusive, in place | Mutate | Unit | current |
| `Set<T>` | `symmetric_difference_update(same kind)` | exclusive, in place | Mutate | Unit | current |
| `Set<T>` | `len()` | borrows | Measure | 0 | current |
| `Set<T>` | `is_empty()` | borrows | Measure | true | current |
| `Set<T>` | `capacity()` | borrows | Measure | 0 or more | current |
| `Set<T>` | `to_list()` | borrows | Build | empty list | current |
| `Set<T>` | `first()` | borrows | Lookup | None | current |
| `Set<T>` | `values()` | borrows | Adapt | empty iterator | current |
| `Set<T>` | `sort()` | borrows | Reorder | empty list | current |
| `Set<T>` | `shuffle()` | borrows | Reorder | empty list | current |
| `Rank<T>` | `add(item)` | exclusive, in place | Mutate | true | current |
| `Rank<T>` | `remove(item)` | exclusive, in place | Mutate | Unit | current |
| `Rank<T>` | `has(item)` | borrows | Measure | false | current |
| `Rank<T>` | `union(same kind)` | borrows | Build | the argument | current |
| `Rank<T>` | `intersection(same kind)` | borrows | Build | empty Rank | current |
| `Rank<T>` | `difference(same kind)` | borrows | Build | empty Rank | current |
| `Rank<T>` | `symmetric_difference(same kind)` | borrows | Build | the argument | current |
| `Rank<T>` | `is_subset(same kind)` | borrows | Measure | true | current |
| `Rank<T>` | `is_superset(same kind)` | borrows | Measure | true only for an empty argument | current |
| `Rank<T>` | `is_disjoint(same kind)` | borrows | Measure | true | current |
| `Rank<T>` | `len()` | borrows | Measure | 0 | current |
| `Rank<T>` | `is_empty()` | borrows | Measure | true | current |
| `Rank<T>` | `to_list()` | borrows | Build | empty list | current |
| `Rank<T>` | `first()` | borrows | Lookup | None | current |
| `Rank<T>` | `last()` | borrows | Lookup | None | current |
| `Rank<T>` | `clear()` | exclusive, in place | Mutate | Unit | current |
| `Bits` | `add(Int)` | exclusive, in place | Mutate | true | current |
| `Bits` | `remove(Int)` | exclusive, in place | Mutate | Unit | current |
| `Bits` | `has(Int)` | borrows | Measure | false | current |
| `Bits` | `clear()` | exclusive, in place | Mutate | Unit | current |
| `Tally<T>` | `add(item)` | exclusive, in place | Mutate | true | current |
| `Tally<T>` | `remove(item)` | exclusive, in place | Mutate | Unit | current |
| `Tally<T>` | `has(item)` | borrows | Measure | false | current |
| `Tally<T>` | `count(item)` | borrows | Measure | 0 | current |
| `Tally<T>` | `any(fn(item) { Bool })` | borrows | Decide | false | current |
| `Tally<T>` | `len()` | borrows | Measure | 0 | current |
| `Tally<T>` | `is_empty()` | borrows | Measure | true | current |
| `Tally<T>` | `clear()` | exclusive, in place | Mutate | Unit | current |
| `Cache<K,V>` | `add(key, value)` | exclusive, in place | Mutate | None | current |
| `Cache<K,V>` | `add_new(key, value)` | exclusive, in place | Mutate | true | current |
| `Cache<K,V>` | `get(key)` | exclusive, in place | Lookup | None | current |
| `Cache<K,V>` | `remove(key)` | exclusive, in place | Mutate | None | current |
| `Cache<K,V>` | `has_key(key)` | borrows | Measure | false | current |
| `Cache<K,V>` | `len()` | borrows | Measure | 0 | current |
| `Cache<K,V>` | `capacity()` | borrows | Measure | the constructed capacity | current |
| `Cache<K,V>` | `is_empty()` | borrows | Measure | true | current |
| `Cache<K,V>` | `keys()` | borrows | Build | empty list | current |
| `Cache<K,V>` | `clear()` | exclusive, in place | Mutate | Unit | current |
| `Queue<T>` | `push_front(item)` | exclusive, in place | Mutate | Unit | current |
| `Queue<T>` | `push_back(item)` | exclusive, in place | Mutate | Unit | current |
| `Queue<T>` | `delete(item)` | exclusive, in place | Mutate | Unit | current |
| `Queue<T>` | `get(Int)` | borrows | Lookup | None | current |
| `Queue<T>` | `split(Int)` | exclusive, in place | Mutate | empty Queue | current |
| `Queue<T>` | `join(String)` | borrows | Build | empty string | current |
| `Queue<T>` | `contains(item)` | borrows | Measure | false | current |
| `Queue<T>` | `len()` | borrows | Measure | 0 | current |
| `Queue<T>` | `capacity()` | borrows | Measure | 0 or more | current |
| `Queue<T>` | `is_empty()` | borrows | Measure | true | current |
| `Queue<T>` | `peek_front()` | borrows | Lookup | None | current |
| `Queue<T>` | `peek_back()` | borrows | Lookup | None | current |
| `Queue<T>` | `to_list()` | borrows | Build | empty list | current |
| `Queue<T>` | `pop_front()` | exclusive, in place | Mutate | None | current |
| `Queue<T>` | `pop_back()` | exclusive, in place | Mutate | None | current |
| `Queue<T>` | `reverse()` | exclusive, in place | Mutate | Unit | current |
| `Queue<T>` | `clear()` | exclusive, in place | Mutate | Unit | current |
| `PriorityQueue<T>` | `remove(item, [RemoveBy])` | exclusive, in place | Mutate | None | current |
| `PriorityQueue<T>` | `push(item)` | exclusive, in place | Mutate | Unit | current |
| `PriorityQueue<T>` | `len()` | borrows | Measure | 0 | current |
| `PriorityQueue<T>` | `is_empty()` | borrows | Measure | true | current |
| `PriorityQueue<T>` | `pop()` | exclusive, in place | Mutate | None | current |
| `PriorityQueue<T>` | `peek()` | borrows | Lookup | None | current |
| `PriorityQueue<T>` | `to_sorted_list()` | borrows | Reorder | empty list | current |
| `PriorityQueue<T>` | `clear()` | exclusive, in place | Mutate | Unit | current |
| `[K:V]` | `add(key, value)` | exclusive, in place | Mutate | None | current |
| `[K:V]` | `try_insert(key, value)` | exclusive, in place | TryMutate | Ok(None) | current |
| `[K:V]` | `add_new(key, value)` | exclusive, in place | Mutate | true | current |
| `[K:V]` | `remove(key)` | exclusive, in place | Mutate | None | current |
| `[K:V]` | `pop(key)` | exclusive, in place | Mutate | None | current |
| `[K:V]` | `get(key)` | borrows | Lookup | None | current |
| `[K:V]` | `has_key(key)` | borrows | Measure | false | current |
| `[K:V]` | `contains_value(value)` | borrows | Measure | false | current |
| `[K:V]` | `pop_first()` | exclusive, in place | Mutate | None | current |
| `[K:V]` | `update(map)` | exclusive, in place | Mutate | Unit | current |
| `[K:V]` | `merge(map, [fn(key, old, new) { value }])` | borrows | MergeEntries | the argument | current |
| `[K:V]` | `equal(map)` | borrows | Measure | true when both are empty | current |
| `[K:V]` | `intersection(map)` | borrows | Build | empty map | current |
| `[K:V]` | `slice([key])` | borrows | Build | empty map | current |
| `[K:V]` | `top_n(Int)` | borrows | Reorder | empty list | current |
| `[K:V]` | `map(fn(key, value) { T })` | borrows | Visit | empty map | current |
| `[K:V]` | `filter(fn(key, value) { Bool })` | borrows | Visit | empty map | current |
| `[K:V]` | `flat_map(fn(key, value) { map })` | borrows | Visit | empty map | current |
| `[K:V]` | `each(fn(key, value) { T })` | borrows | Each | Unit | current |
| `[K:V]` | `any(fn(key, value) { Bool })` | borrows | Decide | false | current |
| `[K:V]` | `all(fn(key, value) { Bool })` | borrows | Decide | true | current |
| `[K:V]` | `fold(seed, fn(acc, key, value) { acc })` | borrows | Fold | the seed | current |
| `[K:V]` | `clear()` | exclusive, in place | Mutate | Unit | current |
| `View<T>` | `map(fn(item) { T })` | borrows | Visit | empty list | current |
| `View<T>` | `fold(seed, fn(acc, item) { acc })` | borrows | Fold | the seed | current |
| `[T]` | `push(item)` | exclusive, in place | Mutate | Unit | current |
| `[T]` | `append(item)` | exclusive, in place | Mutate | Unit | current |
| `[T]` | `try_push(item)` | exclusive, in place | TryMutate | Ok(Unit) | current |
| `[T]` | `try_reserve(Int)` | exclusive, in place | TryMutate | Ok(Unit) | current |
| `[T]` | `insert(Int, item)` | exclusive, in place | Mutate | Unit | current |
| `[T]` | `remove(item, [RemoveBy])` | exclusive, in place | Mutate | None | current |
| `[T]` | `pop()` | exclusive, in place | Mutate | None | current |
| `[T]` | `reverse()` | exclusive, in place | Mutate | Unit | current |
| `[T]` | `fill(Range, item)` | exclusive, in place | Mutate | Unit | current |
| `[T]` | `fill_repeat(Range, [item])` | exclusive, in place | Mutate | Unit | current |
| `[T]` | `clear()` | exclusive, in place | Mutate | Unit | current |
| `[T]` | `get(Int)` | borrows | Lookup | None | current |
| `[T]` | `index_of(item)` | borrows | Lookup | None | current |
| `[T]` | `index(item)` | borrows | Lookup | None | current |
| `[T]` | `contains(item)` | borrows | Measure | false | current |
| `[T]` | `count(item)` | borrows | Measure | 0 | current |
| `[T]` | `update_first(fn(item) { Bool }, item)` | exclusive, in place | Decide | false | current |
| `[T]` | `split_write(Int)` | exclusive, in place | Disjoint | Ok(two empty write views) at zero | current |
| `[T]` | `get_disjoint_write([Int])` | exclusive, in place | Disjoint | Ok(empty view list) for no indexes | current |
| `[T]` | `edit_disjoint([Int], fn(ViewMut<item>, ViewMut<item>) { T })` | exclusive, in place | Disjoint | Err for a selection that does not name two items | current |
| `[T]` | `replace(item, item)` | exclusive, in place | Decide | false | current |
| `[T]` | `binary_search(item)` | borrows | Lookup | None | current |
| `[T]` | `binary_search_by(fn(item) { T })` | borrows | Search | None | current |
| `[T]` | `random()` | borrows | Lookup | None | current |
| `[T]` | `min_max()` | borrows | Lookup | None | current |
| `[T]` | `min_max_by(fn(item) { T })` | borrows | Fold | None | current |
| `[T]` | `sort_by(fn(item) { key or (a, b) -> Ordering })` | exclusive, in place | SortBy | Unit | current |
| `[T]` | `sort_by_desc(fn(item) { key })` | exclusive, in place | SortBy | Unit | current |
| `[T]` | `sort()` | exclusive, in place | Mutate | Unit | current |
| `[T]` | `sort_desc()` | exclusive, in place | Mutate | Unit | current |
| `[T]` | `lazy()` | borrows | Adapt | empty iterator | current |
| `[T]` | `shuffle()` | borrows | Reorder | empty iterator | current |
| `[T]` | `to_set()` | borrows | Build | empty Set | current |
| `[T]` | `concat([item])` | borrows | Build | the argument | current |
| `[T]` | `extend([item])` | exclusive, in place | Mutate | Unit | current |
| `[T]` | `starts_with([item])` | borrows | Measure | true only for an empty prefix | current |
| `[T]` | `ends_with([item])` | borrows | Measure | true only for an empty suffix | current |
| `[T]` | `equal([item])` | borrows | Measure | true when both are empty | current |
| `[T]` | `union([item])` | borrows | Build | the argument's distinct items | current |
| `[T]` | `intersection([item])` | borrows | Build | empty list | current |
| `[T]` | `difference([item])` | borrows | Build | empty list | current |
| `[T]` | `slice(Int, Int)` | borrows | Build | empty list | current |
| `Iter<T>` | `shuffle()` | consumes | Reorder | empty iterator | current |
| `Iter<T>` | `to_list()` | consumes | Drain | empty list | current |
| `Iter<T>` | `collect()` | consumes | Drain | empty list | current |
| `Iter<T>` | `next()` | exclusive, in place | Pull | None | current |
| `[T]`, `Iter<T>` | `map(fn(item) { T })` | borrows a list; consumes an iterator | Visit | empty list or iterator | current |
| `[T]`, `Iter<T>` | `filter(fn(item) { Bool })` | borrows a list; consumes an iterator | Visit | empty list or iterator | current |
| `[T]`, `Iter<T>` | `each(fn(item) { T })` | borrows a list; consumes an iterator | Each | Unit | current |
| `[T]`, `Iter<T>` | `find(fn(item) { Bool })` | borrows a list; consumes an iterator | Decide | None | current |
| `[T]`, `Iter<T>` | `any(fn(item) { Bool })` | borrows a list; consumes an iterator | Decide | false | current |
| `[T]`, `Iter<T>` | `all(fn(item) { Bool })` | borrows a list; consumes an iterator | Decide | true | current |
| `[T]`, `Iter<T>` | `position(fn(item) { Bool })` | borrows a list; consumes an iterator | Decide | None | current |
| `[T]`, `Iter<T>` | `is_sorted_by(fn(item) { T })` | borrows a list; consumes an iterator | Decide | true | current |
| `[T]`, `Iter<T>` | `filter_map(fn(item) { T })` | borrows a list; consumes an iterator | Stream | empty iterator | current |
| `[T]`, `Iter<T>` | `flat_map(fn(item) { T })` | borrows a list; consumes an iterator | Visit | empty list or iterator | current |
| `[T]`, `Iter<T>` | `take_while(fn(item) { Bool })` | borrows a list; consumes an iterator | Stream | empty iterator | current |
| `[T]`, `Iter<T>` | `skip_while(fn(item) { Bool })` | borrows a list; consumes an iterator | Stream | empty iterator | current |
| `[T]`, `Iter<T>` | `dedup_by(fn(item) { T })` | borrows a list; consumes an iterator | Stream | empty iterator | current |
| `[T]`, `Iter<T>` | `chunk_while(fn(item, item) { Bool })` | borrows a list; consumes an iterator | Stream | empty iterator | current |
| `[T]`, `Iter<T>` | `scan(seed, fn(acc, item) { acc })` | borrows a list; consumes an iterator | Stream | empty iterator | current |
| `[T]`, `Iter<T>` | `min_by(fn(item) { T })` | borrows a list; consumes an iterator | Fold | None | current |
| `[T]`, `Iter<T>` | `max_by(fn(item) { T })` | borrows a list; consumes an iterator | Fold | None | current |
| `[T]`, `Iter<T>` | `count_where(fn(item) { Bool })` | borrows a list; consumes an iterator | Fold | 0 | current |
| `[T]`, `Iter<T>` | `reduce(seed, fn(acc, item) { acc })` | borrows a list; consumes an iterator | Fold | the seed | current |
| `[T]`, `Iter<T>` | `fold(seed, fn(acc, item) { acc })` | borrows a list; consumes an iterator | Fold | the seed | current |
| `[T]`, `Iter<T>` | `group_by(fn(item) { T })` | borrows a list; consumes an iterator | Group | empty map | current |
| `[T]`, `Iter<T>` | `count_by(fn(item) { T })` | borrows a list; consumes an iterator | Group | empty map | current |
| `[T]`, `Iter<T>` | `partition(fn(item) { Bool })` | borrows a list; consumes an iterator | Group | two empty lists | current |
| `[T]`, `Iter<T>` | `counts()` | borrows a list; consumes an iterator | Group | empty map | current |
| `[T]`, `Iter<T>` | `unzip()` | borrows a list; consumes an iterator | Group | one empty list per field | current |
| `[T]`, `Iter<T>` | `take(Int)` | borrows a list; consumes an iterator | Adapt | empty iterator | current |
| `[T]`, `Iter<T>` | `skip(Int)` | borrows a list; consumes an iterator | Adapt | empty iterator | current |
| `[T]`, `Iter<T>` | `step_by(Int)` | borrows a list; consumes an iterator | Adapt | empty iterator | current |
| `[T]`, `Iter<T>` | `repeat(Int)` | borrows a list; consumes an iterator | Adapt | empty iterator | current |
| `[T]`, `Iter<T>` | `cycle(Int)` | borrows a list; consumes an iterator | Adapt | empty iterator | current |
| `[T]`, `Iter<T>` | `drop_last(Int)` | borrows a list; consumes an iterator | Reorder | empty iterator | current |
| `[T]`, `Iter<T>` | `chunks(Int)` | borrows a list; consumes an iterator | Window | empty iterator | current |
| `[T]`, `Iter<T>` | `windows(Int)` | borrows a list; consumes an iterator | Window | empty iterator | current |
| `[T]`, `Iter<T>` | `dedup()` | borrows a list; consumes an iterator | Adapt | empty iterator | current |
| `[T]`, `Iter<T>` | `flatten()` | borrows a list; consumes an iterator | Adapt | empty list or iterator | current |
| `[T]`, `Iter<T>` | `indexed()` | borrows a list; consumes an iterator | Adapt | empty iterator | current |
| `[T]`, `Iter<T>` | `indexes()` | borrows a list; consumes an iterator | Adapt | empty iterator | current |
| `[T]`, `Iter<T>` | `intersperse(item)` | borrows a list; consumes an iterator | Adapt | empty iterator | current |
| `[T]`, `Iter<T>` | `first()` | borrows a list; consumes an iterator | Lookup | None | current |
| `[T]` | `last()` | borrows | Lookup | None | current |
| `Iter<T>` | `to_set()` | consumes | Drain | empty Set | current |
| `[T]`, `Iter<T>` | `try_collect()` | borrows a list; consumes an iterator | Drain | Ok(empty list) | current |
| `[T]`, `Iter<T>` | `last_index_of(item)` | borrows a list; consumes an iterator | Lookup | None | current |
| `[T]`, `Iter<T>` | `min()` | borrows a list; consumes an iterator | Lookup | None | current |
| `[T]`, `Iter<T>` | `max()` | borrows a list; consumes an iterator | Lookup | None | current |
| `[T]`, `Iter<T>` | `sum()` | borrows a list; consumes an iterator | Drain | 0 | current |
| `[T]`, `Iter<T>` | `product()` | borrows a list; consumes an iterator | Drain | 1 | current |
| `[T]`, `Iter<T>` | `average()` | borrows a list; consumes an iterator | Drain | 0.0 | current |
| `[T]`, `Iter<T>` | `join(String)` | borrows a list; consumes an iterator | Drain | empty string | current |
| `[T]`, `Iter<T>` | `split(Int)` | borrows a list; consumes an iterator | Build | two empty lists | current |
| `[T]`, `Iter<T>` | `is_sorted()` | borrows a list; consumes an iterator | Measure | true | current |
| `[T]`, `Iter<T>` | `compare([item])` | borrows a list; consumes an iterator | Measure | 0 against an empty list | current |
| `[T]`, `Iter<T>` | `is_empty()` | borrows a list; consumes an iterator | Measure | true | current |
| `[T]`, `Iter<T>` | `len()` | borrows a list; consumes an iterator | Measure | 0 | current |
| `[K:V]` | `len()` | borrows | Measure | 0 | current |
| `[K:V]` | `is_empty()` | borrows | Measure | true | current |
| `[K:V]` | `replace(key, value)` | exclusive, in place | Mutate | None | current |
| `[K:V]` | `keys()` | borrows | Adapt | empty iterator | current |
| `[K:V]` | `values()` | borrows | Adapt | empty iterator | current |
| `[K:V]` | `first()` | borrows | Lookup | None | current |
| `[K:V]` | `min()` | borrows | Lookup | None | current |
| `[K:V]` | `max()` | borrows | Lookup | None | current |
| `[K:V]` | `to_list()` | borrows | Build | empty list | current |
| `[K:V]` | `items()` | borrows | Build | empty list | current |
| `View<T>` | `len()` | borrows | Measure | 0 | current |
| `View<T>` | `is_empty()` | borrows | Measure | true | current |
| `View<T>` | `get(Int)` | borrows | Lookup | None | current |
| `View<T>` | `index_of(item)` | borrows | Lookup | None | current |
| `View<T>` | `first()` | borrows | Lookup | None | current |
| `View<T>` | `last()` | borrows | Lookup | None | current |
| `View<T>` | `contains(item)` | borrows | Measure | false | current |
| `View<T>` | `join(String)` | borrows | Build | empty string | current |
| `Bits` | `len()` | borrows | Measure | 0 | current |
| `Bits` | `count()` | borrows | Measure | 0 | current |
| `Bits` | `is_empty()` | borrows | Measure | true | current |
| `Bits` | `to_list()` | borrows | Build | empty list | current |

<!-- END GENERATED COLLECTION OPERATIONS -->

## `core` — built Core modules

The canonical module registry is [`Core.rows`](../../../Compiler/Prelude/Core.rows),
and the loader's exported list is [`CoreCallRows.jet`](../../../Compiler/JetFoundation/Source/Registry/CoreCallRows.jet), which the compiler build computes from it.
The list below is the source-facing module inventory. A module may be a
namespace-only provider boundary; its presence in the registry does not imply
that every provider operation is available on every target.
The built-module inventory is a source-facing registry, not a missing-domain ledger (`D-STDLIBLEDGER1`). Memory management is opt-in scoped automatic GC through `#Policy(gc)`; collector state remains compiler-private while ordinary code keeps bare owned values (`D-OPTGC1`).

- `app`
- `core`
- `core.devtools`
- `core.debug`
- `core.context`
- `core.actor`
- `core.archive`
- `core.archive.gzip`
- `core.archive.zstd`
- `core.args`
- `core.auth`
- `core.build`
- `core.compiler`
- `core.compiler.lang`
- `core.collections`
- `core.collections.set`
- `core.color`
- `core.compute`
- `core.compute.solve`
- `core.crypto`
- `core.crypto.expert`
- `core.crypto.random`
- `core.crypto.uuid`
- `core.crypto.vault`
- `core.data`
- `core.data.arrow`
- `core.data.loader`
- `core.data.stream`
- `core.data.plot`
- `core.data.sketch`
- `core.data.sketch.cms`
- `core.data.sketch.hll`
- `core.data.sketch.reservoir`
- `core.data.sketch.tdigest`
- `core.db`
- `core.db.sql`
- `core.encoding`
- `core.encoding.base32`
- `core.encoding.base64`
- `core.encoding.binary`
- `core.encoding.cbor`
- `core.encoding.csv`
- `core.encoding.eetf`
- `core.encoding.hex`
- `core.encoding.ini`
- `core.encoding.json`
- `core.encoding.jsonl`
- `core.encoding.toml`
- `core.encoding.xml`
- `core.event`
- `core.files`
- `core.encoding.yaml`
- `core.files.path`
- `core.files.sealed`
- `core.game`
- `core.git`
- `core.grammar`
- `core.grammar.error`
- `core.grammar.plan`
- `core.grammar.runtime`
- `core.grammar.sink`
- `core.http`
- `core.http.client`
- `core.http.server`
- `core.image`
- `core.jobs`
- `core.log`
- `core.math`
- `core.math.random`
- `core.math.combinatorics`
- `core.math.stats`
- `core.mem`
- `core.mem.scope`
- `core.mod`
- `core.net`
- `core.net.mime`
- `core.net.url`
- `core.net.tls`
- `core.net.dtls`
- `core.net.ws`
- `core.net.ip`
- `core.perf`
- `core.plugin`
- `core.prelude`
- `core.process`
- `core.reactive`
- `core.reactive.loadable`
- `core.reflect`
- `core.regex`
- `core.rt`
- `core.service`
- `core.sync`
- `core.sys`
- `core.tasks`
- `core.term` — stdio handles, terminal style, and the `Interaction` provider trait (`TerminalInteraction` default; `ask`, `readline`, `choose`, `confirm`, `secret`, `input_secret`, `progress`, and `prompt_guided` dispatched via `context.interaction` / `#Context(interaction:)`; see [`Examples/Systems/Terminal/InteractionProvider.jet`](../../../Examples/Systems/Terminal/InteractionProvider.jet))
- `core.testing` — deterministic tests, goldens, snapshots, and fake clocks; ambient `assert` renders aligned subexpression values on failure across `O0`/`O1`/`O2` with zero passing-path formatting, and `assert_eq` prints path-level structural diffs for structs, lists, maps, and text
- `core.text`
- `core.text.fmt`
- `core.text.html`
- `core.text.wrap`
- `core.text.parse`
- `core.time`
- `core.time.calendar`
- `core.time.expiring`
- `core.ui`
- `core.tui`
- `core.ui.host`
- `core.ui.host.clipboard`
- `core.ui.host.ime`
- `core.ui.host.drag_drop`
- `core.ui.host.shortcuts`
- `core.ui.host.accessibility`
- `core.ui.xkb`
- `core.ui.font`
- `core.units`
- `core.watcher`
- `core.web`
- `core.browser`
- `core.browser.host`
- `core.browser.storage`
- `core.browser.storage.local`
- `core.browser.storage.session`
- `core.web.devserver`
- `core.web.forms`
- `core.web.query`
- `core.web.router`
- `core.web.store`
- `core.web.table`
- `core.web.virtual`

The sketch types are source-owned plain values: cloning one produces independent
storage, and mutating a sketch uses its `^self` receiver with an explicit `^` call.

### Grammar literals (`core.grammar`)

`core.grammar` exports the `Grammar` checked-text head and `analyze(text, start, imports) -> GrammarPlan GrammarFaults!`. A top-level binding `Name: Grammar{"""…"""}` checks its rule notation while building and generates one struct or enum per parser rule, `{Name}Rule` and `{Name}Token` enums (plus `{Name}Slot` when `HOLE<slot=…>` is present), and methods `parse`, `parse_into`, `print`, and `print_into` (plus `parse_parts` and `check_parts` for hole-bearing grammars).

- `core.grammar.error` exports `GrammarError`, `TextPlace`, `TextSpot`, `GrammarFault`, `GrammarFaults`, and the `GrammarReport` fault variants reported by `Grammar{…}`.
- `core.grammar.plan` exports `GrammarPlan`, `GrammarSource`, `GrammarScanner`, `GrammarPrinter`, and the analyzed rule/lexer/shape tables.
- `core.grammar.sink` exports the `GrammarSink` trait consumed by `parse_into`.
- `core.grammar.runtime` supplies the generated parser, scanner, cursor, and printer runtime helpers.

Core's built-in checked-text heads (`SQL`, `HTML`, `Sh`, `URL`, `DateTime`, `Path`, `Regex`) and format parsers keep their grammars in private `Syntax/` parts under their owning packages so the grammars are checked during builds and deferred unless the owning surface is used. See [`Examples/Text/Grammar/`](../../../Examples/Text/Grammar/) (`Ini.jet`, `Calc.jet`, `LogLine.jet`, `Heads.jet`).

### Two-audience error boundaries (`Describe`)

The ambient Prelude trait `Describe` (`public() -> String Never!` and `detail() -> String Never!`) is implemented on `Err` and Core's own error types (`IOError`, `DBError`, `HTTPError`, `NetError`, `AuthError`, `ServiceError`, `PluginError`, `EncodingError`, `DataError`, `TaskFailure`, `ParseError`, `SendError`, `RangeError`, `TextError`, `UTF8Error`, `EnvError`, `AllocError`, `RegexError`, `TimeError`, `URLError`, `MIMEError`, `WsError`, `ArchiveError`, `CryptoError`, `FileCryptoError`, `UUIDError`, `ModError`, `GitError`, `EventConfigError`, `ImageError`, `AppError`, `CallError`, `Notice`, `BlobError`). Every Core boundary routes `public()` to external callers and `detail()` (with cause chains) to operators:

- `core.http.server` / `core.web`: `HTTPErrorRecord`, `HTTPMux.on_error`, `HTTPMux.error_records`, `HTTPMux.respond_error`, `HTTPServer.on_error`, `HTTPServer.error_records`, `App.on_error`, `App.error_records`, `App.respond_error`.
- `core.args`: `CLIErrorReport` (`from_error`, `log`, `emit`, `exit`), `ArgsSpec.report_error`, `ArgsSpec.fail_described`.
- `core.jobs`: `JobError.from_describe`, `JobQueue.fail_described`, `JobQueue.fail_described_logged`.
- `core.service`: `ToolErrorReport` (`ok`, `from_error`, `caller_json`, `log`).

See [`Examples/ErrorsAndEffects/Errors/TwoAudienceErrors.jet`](../../../Examples/ErrorsAndEffects/Errors/TwoAudienceErrors.jet).

### Writing Core in Jet

Jet source packages under [`Core/`](../../../Core/) are compiled by the Jet
compiler's normal frontend, like any other package.

`Core.rows` is the declaration registry, `Core/**/*.jet` is the source for the
Jet-owned package surface, and provider code supplies operations that require
an external runtime or operating-system capability. A change to an exported
signature must update the source module, the registry, the relevant conformance
corpus, and any examples that exercise the contract. The registry and loader
list are the executable authority; this reference explains the durable rules
and points to those files instead of duplicating their generated details.
The audited intrinsic/ABI kernel is the only provider seam; ordinary Core packages remain Jet source. Archive byte-format calls cross that seam only where the source implementation requires them.

`core.net.tls` separates `tls.connect(host, port)` (which dials TCP and runs a
TLS client handshake) from `tls.client(stream, server_name:, config:, deadline:)`
(which wraps an existing `TCPStream`), and `core.term` exposes `take_bytes`
directly, all as ordinary Jet declarations in `Core/` without dispatcher rows.
An App's route, mount and middleware registrations lower to its Jet `__http_*`
methods after the existing escaping-callback admission. The original effects and
resource conventions remain in force.

Network streams (`TCPStream`, `UnixStream`, `TLSStream`, `DTLSSession`,
`DTLSListener`, and `WsConn`) implement infallible `Close` (`close(handle)` and
scope-exit cleanup) with zero network or wait effects leaked at scope exit, and
separate explicit fallible goodbye operations (`shutdown(.Both)` on `TCPStream`
and `UnixStream`; `finish(...)` on `TLSStream`, `DTLSSession`, `DTLSListener`,
and `WsConn`) so resource ownership comes solely from `impl Close` declarations,
identical to any user-declared `Close` type.

### Database crash consistency and storage model

`core.db` provides whole-or-absent transaction crash recovery (`PRAGMA integrity_check = ok`)
in rollback-journal (`DELETE`/`TRUNCATE`/`PERSIST`) and write-ahead log (`WAL`) modes under
a local POSIX durable storage model with `PRAGMA synchronous` set to `NORMAL`, `FULL`
(default), or `EXTRA`, where the filesystem honors file `fsync` and directory `fsync`
barriers and per-inode write isolation. Between sync barriers, unsynced writes may be
dropped or reordered on crash. Network and remote filesystems (such as NFS, SMB/CIFS,
SSHFS, 9p, GlusterFS, Ceph, Lustre, AFP, and WebDAV) receive **no untested atomicity or
durability promise** and are refused by the crash-consistency harness alongside timeouts,
invalid oracles, unsafe sync settings, and incomplete crash schedules.

### Examples in the repository

Executable examples are organized by capability. Useful entry points for this
part include:

- [`Examples/Text/TextCursor.jet`](../../../Examples/Text/TextCursor.jet)
  and [`binary-reader.jet`](../../../Examples/Data/Binary/BinaryReader.jet)
  for consuming scanners;
- [`Examples/Systems/Files/Files.jet`](../../../Examples/Systems/Files/Files.jet)
  and [`Examples/Systems/CLI/CLIArgs.jet`](../../../Examples/Systems/CLI/CLIArgs.jet)
  for capability-bound I/O;
- [`Examples/Data/JSON/JSONIntegerFidelity.jet`](../../../Examples/Data/JSON/JSONIntegerFidelity.jet)
  for typed encoding behavior;
- [`Examples/Math/Compute/ComputeTensor.jet`](../../../Examples/Math/Compute/ComputeTensor.jet)
  and [`app_live.jet`](../../../Examples/Tooling/DevLoop/AppLive.jet)
  for compute and web surfaces;
- [`Tests/Conformance/Corpus/Core/`](../../../Tests/Conformance/Corpus/Core/)
  for small, checked examples of the exported Core contracts.

Use the source module and its conformance corpus together when documenting an
API: the source establishes the export and error boundary, while the corpus
shows the syntax and boundary behavior that a caller can rely on.
