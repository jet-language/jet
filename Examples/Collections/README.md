# Collections

Lists, maps, sets, queues and the iterator adapters over them.

Run a lesson from the repository root, for example `jet run Examples/Collections/<Lesson>.jet`. Each lesson's expected output sits beside it as `<Lesson>.out`.

## Lessons

- [ArrowOwnership](ArrowOwnership.jet)
- [DynamicArrayView](DynamicArrayView.jet) — A bare range place is a checked read window, `&place` is an exclusive write window, and `place.copy()` makes independent owned storage.
- [FixedArrays](FixedArrays.jet)
- [FnDispatchMap](FnDispatchMap.jet)
- [IndexedMutation](IndexedMutation.jet)
- [IndexHook](IndexHook.jet)
- [IndexOfByValue](IndexOfByValue.jet) — `index_of(item)` on a list or a view returns the position of the first item equal to `item`, or None.
- [KeyedPriorityQueue](KeyedPriorityQueue.jet) — `PriorityQueue.by(key)` pops the item with the smallest key first and `by_desc(key)` the one with the largest; items with equal keys pop in the order they went in.
- [KeyedRank](KeyedRank.jet) — `Rank.by(key)` keeps the item with the smallest key first and `by_desc(key)` the one with the largest; items with equal keys stay in the order they went in, and set operations keep the receiver's order.
- [ListBounds](ListBounds.jet)
- [ListFill](ListFill.jet) — `xs.fill(range, value)` sets a span of a list to one value, and `xs.fill_repeat(range, pattern)` tiles a pattern across it.
- [ListFillBounds](ListFillBounds.jet) — `fill` does not clamp: a range that runs past the list stops with the same bounds fault as `xs[range]`.
- [ListPredicateOps](ListPredicateOps.jet) — List predicate traversal and first-match replacement.
- [ListRangeSlice](ListRangeSlice.jet) — List range slices copy the selected items.
- [ListRemoveSelector](ListRemoveSelector.jet) — A literal `.Slot` or `.Val` selector picks the removal form at compile time.
- [Lists](Lists.jet)
- [MapCompositeKey](MapCompositeKey.jet) — Tuples and structs with key-eligible fields are value-semantic ordered map keys.
- [MapMerge](MapMerge.jet) — Map.merge - right wins by default; optional conflict callback.
- [MapSetMethods](MapSetMethods.jet) — Every Map and Set method once, the in-place `update` forms among them (`m.update(other)`, `s.update(t)`, `s.difference_update(t)` and so on).
- [MapsNested](MapsNested.jet)
- [MemberSpread](MemberSpread.jet) — Member spread expands a shared prefix into field reads.
- [OperationContract](OperationContract.jet) — Each collection operation follows its checked row in the operation contract (Docs/spec/reference/core-library.md).
- [PopTable](PopTable.jet) — Pop removes and returns from each collection.
- [Queue](Queue.jet) — Queue<T> — ring-buffer double-ended queue.
- [ReceiverMethodSurface](ReceiverMethodSurface.jet) — Map, Set, List, View and Iter receiver methods from the canonical collection table.
- [Rows](Rows.jet) — `Rows<T>` is an add-only table of rows reached through the 4-byte `Row<T>` handles `add` returns.
- [Set](Set.jet) — Set<T> — hash-backed unordered set.
- [Sketch](Sketch.jet) — core.data.sketch — approximate data structures.
- [SortByKey](SortByKey.jet) — sort_by and sort_by_desc reorder the marked list in place by a key closure.
- [TupleOrder](TupleOrder.jet) — Named tuples order member by member in written order.
- [WordCount](WordCount.jet)

## Iterators

- [FilterMap](Iterators/FilterMap.jet) — filter_map and try_collect — failure-aware collection adapters.
- [FoldAssociation](Iterators/FoldAssociation.jet)
- [FromKeepsSource](Iterators/FromKeepsSource.jet) — Building a collection from a List reads the List: `Set.from(xs)`, `xs.to_set()` and `PriorityQueue.from(xs)` copy the elements, and the source binding keeps…
- [Generators](Iterators/Generators.jet) — A generator returns `Stream<T>` and uses `yield` to hand a value to the consumer, suspending until the next pull.
- [HeldCursor](Iterators/HeldCursor.jet) — A writable, held user-defined Iterator cursor yields successive items.
- [IterAdapters](Iterators/IterAdapters.jet) — Concrete collection map/filter are eager and return plain collections.
- [IterHook](Iterators/IterHook.jet)
- [IterResume](Iterators/IterResume.jet) — `next` pulls one item from the same source and leaves the exact remainder.
- [IterStreams](Iterators/IterStreams.jet) — Every Sequence method takes an Iter too, and runs as the items are pulled: nothing is collected first, and `find` stops pulling at its answer.
- [IterToolsAudit](Iterators/IterToolsAudit.jet) — Collection breadth and iterator adapter audit.
- [ZipFamily](Iterators/ZipFamily.jet)
