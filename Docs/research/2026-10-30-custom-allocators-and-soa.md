# Custom allocators and struct-of-arrays: what Jet implements today (2026-10-30)

Tower card: #6314. Related cards: #5030 (context allocator), #2889 (column storage), #4169, #6310. Ballots drafted from this research: `D-ALLOC-TRAIT1`, `D-ALLOC-SITE1`.

Code facts cite dev `82d3369757`. Probes used the prebuilt dev compiler `/mnt/jetscratch/discovery/base/latest/jet` (also `82d3369757`) with `jet run` on all three tiers: `--optimize=none` (o0), `--optimize=basic` (o1) and `--optimize=full` (o2). Every probe gave the same result on all three tiers.
Probe files live in `/mnt/jetscratch/lanes/MemDesign/probes/`, with per-tier output in `probes/out/` and summaries in `probes/summary*.tsv`. Reasoning that no probe tested is marked `[INFERENCE]`.

## Plain summary

The owner asked four questions about custom allocators, and how far struct-of-arrays (SoA) storage goes.

1. **Can a user write their own allocator today?** Not one that Jet uses. A user can write an allocator-like type in plain Jet, such as a slot pool with handles, and it works on every tier. But no allocator trait exists, so nothing in Jet will ask that type for memory. Lists, maps and text always take memory from the runtime heap. Even Jet's own arenas cannot be passed to a function or stored in a struct.
2. **What does it interfere with?** Ownership and the region rule for views, the `Mem` effect row, the `try_` failure family, and drop order at reset and close. Also tasks, C calls that free what Jet allocated, Close resources, the GC opt-in, compile-time values, and the rule that all tiers agree.
3. **What does it overlap with?** The four built-in allocators (`Arena`, `Bump`, `Pool`, `Fixed`), the program allocator (`mem.Heap`, `mem.Counting`), the `#Context(allocator:)` block and the ratified `context.allocator` and `context.scratch`, and the freestanding allocator fact.
4. **Can Jet support it natively?** Yes. Two ratified decisions already assume "the core allocator trait", and the runtime heap is already written in Jet. What is missing is the trait itself, a way to carry an allocator value at run time, and a collection header with room to remember its allocator.

SoA: `#Layout(columnar)` parses, type-checks, and reports `columnar` as a layout fact, but it changes nothing at run time. The o2 binary of a columnar program matches the plain program byte for byte except for line numbers, and the loop strides over whole 64-byte structs. Every list method works on a columnar list because it is an ordinary list, and the E1108 "not supported on a columnar list" checks never fire. D-SOA-SITE1 A, ratified, replaces the marker with `Columns<T>`, which does not exist in code.

## 1. Allocators in Jet today

**A closed set known by name.** `Core/Mem/Mem.jet:23-31` declares empty structs `Arena`, `Bump`, `Fixed`, `Pool<T>` and `Pin<T>`, which the comment calls compiler-owned. Sema recognizes an allocator by its type name. `sema_call_memory_allocator_name` (`Compiler/JetSema/Source/Sema/Calls/Memory.jet:43-46`) returns a name only for `Arena`, `Bump`, `Pool` and `Fixed`. `.alloc(value)` and `.try_alloc(value)` return a view tagged `AllocatorView` (`Memory.jet:289-300`). The runtime reserves the value's bytes in the allocator and records a drop entry (`Compiler/JetRuntime/Source/MemStorage.jet:15-21`, `storage_arena_reserve` at :181).

**What works**, on o0, o1 and o2:

| Probe | Prints | Shows |
|---|---|---|
| `a11_arena_local_use.jet` | `45` | `arena.alloc(i)` in a loop, then `close(^arena)` |
| `a4_alloc_list_in_arena.jet` | `3`, `hello` | a list and a text placed in an arena, then reset |
| `a6_arena_in_task.jet` | `4950` | an arena local to a called function |
| `a12_user_pool.jet` | `false`, `true`, `gamma`, `true`, `1` | a user-written generic slot pool with generation checks, in safe Jet |

**What fails**, on o0, o1 and o2:

| Probe | Result | Shows |
|---|---|---|
| `a7_allocator_param.jet` | E0102 `Arena` has no method `alloc` (line 6) | an arena passed as a parameter loses its methods |
| `a7b_allocator_ref.jet` | same E0102 | the same for `&mem.Arena` |
| `a8_allocator_field.jet` | same E0102 (line 9) | the same for an arena in a struct field |
| `a5_pool_typed.jet` | E0333 comparison chain changes direction | `mem.Pool<Node>.new(...)` does not parse as a call |
| `a2_list_with_allocator.jet` | E0107 nothing named `List` | there is no `List.new(allocator:)` |
| `a10_6310.jet` | E-ERR-PROPAGATE | `try_alloc` failures propagate implicitly; `?` is refused |

So an allocator works only on the binding where it was made. Library code cannot take one. [INFERENCE: the parameter's checked type is not the bare `Named("Arena")` that `Memory.jet:44` tests for.]

**No trait, and two silent holes.** No `Allocator` trait exists in Core, the Prelude or the compiler. Two probes show that the compiler does not notice:

- `a1d_unknown_trait.jet` writes `impl Counted.NoSuchTraitAnywhere { ... }`. It compiles and runs on every tier, and the methods become ordinary methods. An impl of a trait that does not exist should be an error.
- `a1e_trait_call.jet` declares `fn takes(a: mem.Allocator)`. The unknown type is accepted in the signature. Only the call fails, with E0112 "`takes` wants `mem.Allocator` ... but this is `Arena`".

**Collections ignore allocators.** A list header is 24 bytes: length, capacity and data. `coll_list_push` (`Compiler/JetRuntime/Source/Collections.jet:186-202`) grows the buffer with `jet_rt_alloc` and frees the old one with `jet_rt_free`, the global runtime heap. The header has no slot for an allocator. A list placed in an arena by `arena.alloc(list)` puts its header in the arena while its elements stay on the heap [INFERENCE from the reserve size and drop entries].

**`#Context(allocator:)` has no effect.** `a3_context_list.jet` makes `mem.Arena.new(capacity: 64)` and pushes 10,000 integers inside `#Context(allocator: arena)`. It prints `10000` on every tier. The 80,000 bytes came from the heap. `Compiler/JetIR/Source/BuildTransact.jet:223-236` says the other guards "have no native carrier: their values run and are released"; only `deadline` is carried. The marker is declared at `Compiler/Prelude/Markers.jet:166`. Card #5030 tracks this.

**The program allocator is a closed pair.** D-ALLOC-PROGRAM1 A says the `package.jet` `allocator:` fact "names any value whose type implements the core allocator trait". The real parser (`Jetpack/PackageModel/Source/Manifest/Blocks/Allocator.jet:11-23`) accepts only `mem.Heap` and `mem.Counting.over(mem.Heap, cap: ...)`.

**The runtime heap is Jet code.** `Compiler/JetRuntime/Source/Alloc.jet` is a size-class allocator over `mmap` with per-arena slabs. So the code a user-written allocator would need, raw pages and atomic words, already exists in audited `#Unsafe` Jet. `mem.from_addr`, the public raw-address entry point, still panics (`Core/Mem/Mem.jet`, INTERIM #4971).

## 2. The owner's four questions

**Q1. Can a user write their own allocator today?**
A user can write a pool, free list or arena of values in plain safe Jet, as `a12_user_pool.jet` shows: a list of values, a list of generations, and handles. That is how games use index pools. A user cannot write an allocator that lists, maps or text will use. No trait exists, collections call the runtime heap directly, and `#Context(allocator:)` is ignored. A user also cannot write raw-byte allocators, since `mem.from_addr` panics and no public byte-block type exists.

**Q2. What would a user allocator interfere with?**
- *Ownership and regions.* A view into an arena must not outlive the arena (E0631, D-ALLOC2 and D-REGION1). A user allocator's memory needs the same rule, or its values can dangle.
- *Effects.* Every allocator call records a `Mem` effect (`Memory.jet:248-256`). A user allocator's methods must carry the same row.
- *Failure.* D-ALLOCFAIL1 A makes `try_` calls return `AllocError`. A user allocator must report failure the same way, and `AllocError.allocator` names it.
- *Drops.* Arenas run each value's drop exactly once, in reverse order, at reset and close (`MemStorage.jet:1-4`). A user allocator that frees without dropping would leak or double-free owned values.
- *Tasks.* Under D-JAI-CONTEXT1 A a spawned task sees its parent's context. An allocator used from two tasks must be safe for that, or Jet must refuse to share it.
- *C calls.* Under D-FFI-CAP1 A a value given to C with `^` is freed by C, so it must live in C's malloc memory (D-ALLOC-DEFAULT1 detail).
- *All tiers agree (I9).* A user allocator runs as Jet code, so every tier runs the same code. The interpreter and native tiers must route the same allocations through it.
- *Close resources.* An arena is a Close resource and must end with `close(^arena)` (`Examples/features/memory/allocator_ctor_args.jet`). A user allocator that owns memory needs the same rule. A collection that remembers its allocator keeps that allocator in use, so closing or resetting it must wait for the collection, as E0631 and E0632 already require for views.
- *The GC opt-in.* Under D-OPTGC1 A, values inside a `#Policy(gc)` scope gain traced identity through the collector's own allocation entry (`Docs/spec/architecture.md:1202-1208`). An allocation there cannot also come from a user allocator. Either the scope ignores `context.allocator`, or Jet refuses a non-heap allocator inside it. The native runtime does not carry the collector yet.
- *Compile-time code.* Values computed while building (D-COMPTIME-WORD1 A) are made by the compiler's evaluator and copied into the program. A user allocator's raw-memory code would not run there, so compile-time values cannot come from it [INFERENCE: no probe ran `#Unsafe` code at compile time].
- *The runtime heap.* A user allocator gets its pages from somewhere: `mem.Heap`, a fixed buffer (D-FIXED-BACKING1) or the OS. Lists made under the default context still use the runtime heap, so both run side by side.

**Q3. What does it overlap with?**
- The four built-in allocators. Under D-EXTEND-ARCH1's "Core gets no private powers" rule, they would become ordinary implementations of the same trait.
- The program allocator, `mem.Heap` and `mem.Counting.over` (D-ALLOC-DEFAULT1 A, D-ALLOC-PROGRAM1 A).
- `#Context(allocator:)`, and the ratified readable `context.allocator` and per-task `context.scratch` arena (D-JAI-CONTEXT1 A). That decision says "an unannotated allocation uses context.allocator, which defaults to the program allocator". It chose the implicit context over Zig's explicit allocator parameters.
- The freestanding allocator fact (D-TARGET-ALLOC1 A), such as `allocator: board.allocator.none`.
- `mem.Pool<T>` and plain index pools like `a12_user_pool.jet`, which solve the same problem with handles instead of addresses.

**Q4. Can Jet support it natively?**
Yes, and ratified law already expects it. D-ALLOC-PROGRAM1 A and D-JAI-CONTEXT1 A both need a value of "any allocator" type, which only a trait can give. Three things are missing, and none needs new syntax:
1. The trait. Its shape is the owner choice in ballot `D-ALLOC-TRAIT1`.
2. A run-time carrier for an allocator value. Today an allocator is a compiler-known name, and the context block drops it (`BuildTransact.jet:223-236`).
3. Collections that remember their allocator. The list header needs an allocator word, and growth and free must call through it (`Collections.jet:186-202`).

Where an allocator is chosen, by context only or also by an explicit argument, is ballot `D-ALLOC-SITE1`.

Prior art. Zig passes `std.mem.Allocator`, a pointer plus a table of `alloc`, `resize`, `remap` and `free`, to every function that allocates (ziglang.org/documentation/master/std/#std.mem.Allocator). Odin keeps an allocator procedure in an implicit `context` and lets `make` take one explicitly (odin-lang.org/docs/overview/#allocators). Jai pushes allocators through its context. Rust's `Allocator` trait with `Vec::new_in(alloc)` has been unstable since 2016 (github.com/rust-lang/rust/issues/32838). C++ has allocator template parameters and the polymorphic `std::pmr::memory_resource`.

## 3. Struct-of-arrays

**The marker parses but does nothing at run time.** `#Layout(columnar)` sets `StructLayout.Columnar` in the parser (`Compiler/JetParser/Source/Parser/DeclarationMarkers.jet:156,218`) and `TStructLayout.Columnar` in sema (`Registration/Checked.jet:336`). Comptime reflection reports it (`Comptime/Reflection.jet:696-698`), and `comptime/layout_facts` prints `columnar`. The IR never reads it. `perf/A/soa.jet` (plain) and `perf/C/soa.jet` (columnar) are the same eight-field `Body` list summed over one field. Built at o2 under the same file name, the two binaries differ in 16 bytes, and their disassemblies differ only in two line-number constants (`mov $0xe` against `mov $0xf`). At o0 they differ in 13 bytes and at o1 in 16. The o2 loop computes `index << 6`, the 64-byte struct size, and loads `mass` at offset `0x30`. That is array-of-structs. Both run in 354 to 386 ms.

**Every list method works on a columnar list.** `probes/api/` runs 22 operations on a columnar list and on the same list without the marker. On every tier, both give the same output for map, filter, sort_by, insert, get, slicing, clear, pop, first and last, reverse, each, any, find, copy, element swap, `==`, and printing. Field write `ps[1].x = 30.0`, index write and `push` also work (`s1b_columnar_ops.jet`). Where an operation fails, it fails the same way with or without the marker, because the probe's spelling was wrong.

**The E1108 checks are dead.** Sema has E1108 checks for list methods (`Calls/Collections.jet:202-205`), slicing (`Expressions/Literals.jet:998-1000`), field writes (`Statements.jet:5216-5222`) and index writes (`Statements.jet:5328-5331`). None fires in any probe. `s12_method.jet` calls `.map` on a columnar list and prints `[2.0]`. The UI snapshot `tests/ui/layout_columnar_unsupported_method.stderr` expects E1108, but the compiler reports E0202 there, because the probe list lacks `&`. With `:=` the same program runs. E1109 (partial columnar) is unreachable as well: `#Layout(columnar: x)` fails earlier, with E0930 marker arguments. E1107 (`columnar [T]`) is unreachable too: the parser reports E0335 first (`s5_prefix.jet`).

**Card #2889's 15x slowdown is gone, for the wrong reason.** That card measured the old Rust columnar path. Today columnar code is plain code, so it costs nothing and gains nothing.

**Ratified law.** D-SOA-SITE1 A (2026-10-02) retires `#Layout(columnar)` in favor of `Columns<T>`. That is a Core collection with the list surface and a column view per field (`ps.y`, an ordinary slice), plus D-SOA2D's rule that JSON stays the same. Its performance gate is a paired cell against an array-of-structs list on the #2889 workload. D-SOA-TIER1 A gives every tier one Prelude column store and one gather symbol. D-ACCEL1 A lets the optimizer make column copies by itself behind a measured gate. No `Columns<` exists in code. `s3_columns_type.jet` fails to parse its literal (E0003), and `s8_column_view.jet` fails with E0302 on `ps.x`.

**SoA beyond whole structs.** Partial SoA (hot fields in columns, cold fields in a side table), blocked layouts (AoSoA, eight records per block) and nested columns are real techniques. Odin's `#soa` and Zig's `std.MultiArrayList` stop at whole structs, as `Columns<T>` does (odin-lang.org/docs/overview/#soa-data-types). Jai's 2015 demos showed an `SOA` modifier on whole structs and arrays. Jai has no public compiler, so this research could not check its current form. Jet has no whole-struct column storage to measure yet, so no evidence shows a workload that needs more. This research drafts no ballot for SoA beyond D-SOA-SITE1.
