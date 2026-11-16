# Memory

Ownership, views, shared state, allocation and the unsafe tier.

Run a lesson from the repository root, for example `jet run Examples/Memory/<Lesson>.jet`. Each lesson's expected output sits beside it as `<Lesson>.out`.

## Lessons

- [ComputedField](ComputedField.jet) — A marked computed field keeps its answer until a stored sibling in its dependency graph changes.
- [CopyVerb](CopyVerb.jet) — `x.copy()` — the one copy sigil.
- [DeferConsume](DeferConsume.jet) — `defer close(resource)` consumes its resource when the scope exits.
- [EffectDenials](EffectDenials.jet) — Memory floors are effect denials.
- [EntityTree](EntityTree.jet)
- [EntityWorld](EntityWorld.jet) — Pools: `Pool<T>`/`Id<T>` — a generational arena.
- [ExpiringSecret](ExpiringSecret.jet)
- [LocalCache](LocalCache.jet)
- [Memoize](Memoize.jet) — Pure function-result memoization has one bounded default and one explicit unbounded spelling.
- [MethodFootprint](MethodFootprint.jet) — Sema infers the record fields each method reads or changes through `self`; an expert may state them on the receiver with `&self.field` or the member spread…
- [Ownership](Ownership.jet) — Ownership: unmarked is read by default, `&` to write/change, `^` to move, `x.copy()` to copy.
- [ParameterModes](ParameterModes.jet)
- [Pin](Pin.jet) — `Pin<T>` is the address-stability contract.
- [ReadParamConst](ReadParamConst.jet) — An unmarked parameter is Jet's const: deep, call-long, no-copy read access, even when the caller's binding is mutable.
- [ReceiverMarks](ReceiverMarks.jet) — Mark the value, not the call.
- [ResourceClose](ResourceClose.jet) — Explicit deferred close transfers ownership beside acquisition and runs once in reverse order when the scope exits.
- [ScopeCloseSafetyNet](ScopeCloseSafetyNet.jet) — Automatic scope-end cleanup is the safety net.
- [ScopeCloseSafetyNetPanic](ScopeCloseSafetyNetPanic.jet) — A panic still runs the automatic scope-end close for every live `Close` local of the stopping function before the process stops, in reverse declaration order.
- [ScopeGuard](ScopeGuard.jet) — Scope-exit cleanup via `core.mem.scope.guard`.
- [StreamStore](StreamStore.jet) — `mem.stream_store(dst, value)` is `dst = value` written without filling the cache.
- [TakeLastUse](TakeLastUse.jet) — Call-site moves: an unmarked argument to a take (`&&T`) parameter moves when the call is the name's last use, exactly as `name.move()` would.

## Allocation

- [Arena](Allocation/Arena.jet) — Explicit arena allocators under `core.mem`.
- [ArenaRegions](Allocation/ArenaRegions.jet) — Real bump-allocated arenas with scope-bound regions.
- [CacheLinePadding](Allocation/CacheLinePadding.jet) — `#build.cpu.cache_line` is the cache-line size of the selected CPU in bytes.
- [ContextAllocator](Allocation/ContextAllocator.jet) — A user allocator serving a block: inside `#Context(allocator: a)`, the values the code makes come from `a`.
- [FixedBacking](Allocation/FixedBacking.jet) — `mem.Fixed.over(bytes)` allocates typed values inside a caller-owned buffer.
- [GCCyclic](Allocation/GCCyclic.jet) — Scoped promotion traces nested ownership and cyclic mutations while the source keeps ordinary bare-value syntax.
- [PoolStaleID](Allocation/PoolStaleID.jet) — Pools: a stale `Id<T>` — its pool slot was removed — panics at runtime, mirroring the array-out-of-bounds panic precedent (jet_index_vec/jet_pool_get): a…
- [ProgramAllocator](Allocation/ProgramAllocator/) — package.jet wraps the hidden system heap once for the whole hosted program.
- [TryAllocation](Allocation/TryAllocation.jet) — Fallible allocation is a value.
- [Uninit](Allocation/Uninit.jet) — `name := Type{ uninit }` opts out of automatic zero-fill for a binding, skipping the default initialisation for raw performance.
- [UserAllocator](Allocation/UserAllocator.jet) — A user-written allocator: `mem.Allocator` over a `mem.Region`.

## Shared

- [SharedConfig](Shared/SharedConfig.jet)
- [SharedFieldChain](Shared/SharedFieldChain.jet)
- [SharedFieldClose](Shared/SharedFieldClose.jet)
- [SharedFieldWrite](Shared/SharedFieldWrite.jet)
- [SharedGuardProjection](Shared/SharedGuardProjection.jet) — Map and split an edit guard into direct fields.
- [SharedGuardQueue](Shared/SharedGuardQueue.jet)
- [SharedSame](Shared/SharedSame.jet)
- [SharedTransact](Shared/SharedTransact.jet)
- [SharedWeakCycle](Shared/SharedWeakCycle.jet)
- [ShareOnReuse](Shared/ShareOnReuse.jet) — An unmarked use gives a value away only when it is the name's last use.

## Unsafe

- [UnsafeSentries](Unsafe/UnsafeSentries.jet) — Default dev-tier sentries quarantine and poison an arena allocation before a raw pointer can read it again.
- [UnsafeSentriesPackageOff](Unsafe/UnsafeSentriesPackageOff/) — Package policy mirrors source policy and disables only the runtime witness for this package.
- [UnsafeSentriesProvenance](Unsafe/UnsafeSentriesProvenance.jet) — A raw write from an address with no allocation provenance is a located R0801 runtime fault in the default dev tier.
- [UnsafeSentriesSourceOff](Unsafe/UnsafeSentriesSourceOff.jet) — Source policy can turn off only the runtime witness.

## Views

- [PlaceWindows](Views/PlaceWindows.jet) — A particle/grid update using bare reads, `&` edits, and `.copy()` copies.
- [ProjectedWriteWindows](Views/ProjectedWriteWindows.jet) — `&place` is the exclusive write window for any place, a name followed by field or index projections.
- [ReturnedViews](Views/ReturnedViews.jet) — Named View boundaries carry inferred owner provenance; `.copy()` materializes a view at an output boundary that requires Display.
- [StringView](Views/StringView.jet) — Views: `.trim()`/`.after(sep)`/`.before(sep)` bound to a local return a zero-copy view — a genuine `&str` borrow into the receiver's own buffer, not a fresh…
- [ViewFromParam](Views/ViewFromParam.jet)
- [ViewMoveCopy](Views/ViewMoveCopy.jet) — The compiler chooses when a name is unmarked, and a mark gives exact control.
- [ViewReturnLifetimes](Views/ViewReturnLifetimes.jet) — A returned view borrows the owners sema proved for it, and every tier keeps that relation.
- [ViewSubranges](Views/ViewSubranges.jet) — A subrange of a declared view reborrows that view's owner.
