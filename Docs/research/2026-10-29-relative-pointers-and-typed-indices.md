# Relative pointers and typed small indices: measured, with an erasure design (2026-10-29)

This dated research answers the owner's request of 2026-10-29 on card #6204
(`c00mjxx6`). He asked about relative pointers and their benefits, and whether
their types can live at compile time and erase to zero-cost run-time integers.
His example is a node `{char text[32]; uint16_t left; uint16_t right;}` in a
fixed pool of 1024, linked by 16-bit indices. This report is evidence for the
array-programming package ballot. It does not ratify anything or own any work.

Every web source below was read on **2026-10-29**. The benchmarks ran the same
day on one shared machine: an AMD Ryzen 9 7950X3D (16 cores, 32 threads) with
64 GB of RAM. Its load average stayed between 39 and 52 throughout, so every
timing is noisy (section 2.7). C was built with GCC 15.3.0 at `-O2`. Jet was
built with `jet build --release` from toolchain identity
`jet@0.0.1#9c7a98e2daec`. Repository citations are to dev `e23bd7f503`.

Measured facts, probe results and quoted sources are kept apart from inference.
Inference is labelled **[INFERENCE]**.

## Plain summary

1. There are two kinds of small pointer. A **self-relative** pointer stores the
   distance from its own address to the target, as in Jai's `*~s32`. A
   **base-relative** pointer, or index, stores an element number in a known
   array (section 1).
2. In the owner's pool a node is 36 bytes with 16-bit indices, 40 with 32-bit
   self-relative offsets, and 48 with 64-bit pointers. The pool is 36,864,
   40,960 and 49,152 bytes respectively (section 2.2).
3. A 16-bit self-relative offset cannot span the owner's pool. It reaches 32,767
   bytes, which is 910 nodes of 36 bytes. A 16-bit index reaches 65,536 nodes
   of any size.
4. Copying the whole pool: indices and self-relative offsets copy with one
   `memcpy`. Pointers also need a fixup pass, which made a copy 3.0 times slower
   at 1,024 nodes and 5.5 times slower at 65,535 (section 2.4).
5. Lookup speed did not separate the three encodings reliably on this loaded
   machine. Pointers were fastest at 1,024 nodes. At 65,535 nodes the ranking
   flipped between two sessions (section 2.3).
6. A pool built into a position-independent C binary needs 2,051 load-time
   relocations when linked by pointers and none for its links when linked by
   indices. The index pool lands in read-only data (section 2.5).
7. Jet today is 4 to 8 times slower than C on the same lookup loop and 11 to 15
   times slower on pool copies. A Jet list of these nodes peaks at about 116
   bytes per node while it grows (section 2.6).
8. Jet's range types already erase. A `distinct U16` field takes 2 bytes, and
   `distinct Int(0..3)` indexes a `[T#4]` inside a function that may not stop
   (section 5.2).
9. Four gaps stand in the way of the owner's example in Jet today. A range type
   cannot use a `U16` carrier (E0137). A proof is lost through a match payload
   (E0749). A compile-time list constant crashes the backend. `[T#N].filled` is
   not built (section 5.3).
10. Section 6 proposes one erasure design built from `distinct` range types. A
    `NodeIx :: distinct U16(0..1023)` is a bare 2-byte integer at run time, and
    indexing a `[Node#NodeIx]` with it never checks and never stops.
11. Self-relative `Rel<T, I32>`, already ratified for `mem.Image`, stays the tool
    for position-independent images. For pools the measurements favour typed
    indices (section 7).

## 1. Two kinds of small pointer

### 1.1 Self-relative

A self-relative pointer stores `target address − own address`. Following it is
one add to the field's own address. The stored value is valid wherever the
containing bytes land, as long as the target moves with them.

- **Jai** has a relative pointer type. Jet's Jai audit records it as a 32-bit
  self-relative offset declared with `~` and an explicit `s32`. Zero is null,
  and the encoding XORs the high bit (`Docs/audits/jai-parity-2026-10-16.md:309`,
  `:640`, from videos `GQj8fzxpdrk` 00:03:11 and `_HGkhvpCbsk` 01:36:20). The
  owner's spelling `*~s16` is the 16-bit form.
- **Cap'n Proto** struct and list pointers hold a "30 bits ... Offset, in words,
  from the end of the pointer to the start of the struct's data section.
  Signed." (`capnproto.org/encoding.html`).
- **FlatBuffers** refers to tables, strings and vectors by `uoffset_t`, "currently
  always a `uint32_t`". Offsets point forward: "Any backwards offsets will be
  explicitly marked as such" (`flatbuffers.dev/internals/`). FlexBuffers stores
  "an unsigned integer indicating the number of bytes in a negative direction
  from the address of itself".

### 1.2 Base-relative (indices)

An index stores an element number in a known array. Following it needs the
array's base address: `base + index × element size`. The value is valid in any
copy of the array, wherever the array lands.

- **Andre Weissflog** (floooh), "Handles are the better pointers", 2018-06-17,
  updated 2018-11-28. He recommends index handles into arrays owned by one
  system. His example is the owner's size: 16-bit handles for 1,024 live items,
  so 10 index bits and 6 bits of "unique pattern" that detect dangling handles.
  He notes handles can be shared between a game server and its clients, and
  stored in save games.
- **slotmap** (Rust crate docs, version 1.1.1): keys are an index plus a
  version. A slot costs `4 + max(sizeof(T), 4)` bytes, rounded up to `T`'s
  alignment. A map holds up to 2^32 − 2 elements, and a slot's version wraps
  after 2^31 reuses.
- **typed_index_collections** (Rust crate docs): `TiSlice<K, V>` and `TiVec<K,
  V>` index only by a key type `K` that converts to and from `usize`. Indexing
  with the wrong key type is a compile error.
- **Zig's compiler** stores its syntax tree and intern pool as indices.
  `lib/std/zig/Ast.zig:39-40` declares `OptionalTokenIndex = enum(u32) { none =
  std.math.maxInt(u32), _ }`, and `Node.OptionalIndex` repeats the pattern
  (`:3038-3041`). `src/InternPool.zig:2083-2084` has a 16-bit one: `VectorIndex
  = enum(u16) { none = std.math.maxInt(u16), _ }`. These are the Zig master
  files as fetched on 2026-10-29.
- **rustc** declares its indices with `newtype_index!`. The default maximum is
  `0xFFFF_FF00`, to "shave off 256 indices at the end to allow space for packing
  these indices into enums" (`compiler/rustc_index_macros/src/newtype.rs:102-103`).
  Its docs add that a lower `#[max]` "allows niche optimizations" (`lib.rs:33-34`).
- **Odin**: the current overview, read 2026-10-29, documents no relative pointer
  type. Whether an earlier Odin had one was not verified.

### 1.3 How the two compare

| Property | Self-relative | Index |
|---|---|---|
| Follow | field address + offset | pool base + index × size |
| Needs at the use | nothing else | the pool |
| Reach of 16 bits | ±32 KiB of bytes | 65,536 elements of any size |
| Copy the whole region | valid | valid |
| Copy one holder elsewhere | target changes unless adjusted | valid if the pool is the same |
| Free null value | 0 (a field never points at itself) | none: reserve one value |
| Type tells which region | no region needed | the index type must name the pool |

The owner's 36-byte node shows the reach difference. A signed 16-bit byte
offset covers ±32,767 bytes, so a link can reach 910 nodes on either side.
Links in a 1,024-node pool can be up to 36,828 bytes apart. A 16-bit index
covers all 1,024 with 64 times headroom.

## 2. Measurements

### 2.1 Method

The C benchmark (`pool2.c`, appendix A) builds a binary search tree in a fixed
pool three ways: 64-bit pointers (`ptr`), 16-bit indices (`idx`), and 32-bit
self-relative byte offsets (`rel`). All three use the same names (`node-<k>`),
the same shuffled insertion order and the same key stream. One process runs one
encoding. Runs of the three encodings are interleaved, five times at 1,024 nodes
and three times at 65,535.

The key compare is 32 bytes, word by word through `bswap64`. A `-DBYTECMP`
build compares byte by byte, which is the loop the Jet version writes.

The Jet benchmark (`pool.jet`, appendix B) builds the same kind of tree in a
`[Node]` list with `U16` links, with the same names and the same lookup loop.
Its compare is the byte loop. Its random generator differs from the C one, so
the two trees have different shapes of the same size.

### 2.2 Sizes

| Encoding | Node | 1,024-node pool | 65,535-node pool |
|---|---:|---:|---:|
| `ptr` (64-bit pointers) | 48 B | 49,152 B | 3,145,680 B |
| `idx` (16-bit indices) | 36 B | 36,864 B | 2,359,260 B |
| `rel` (32-bit self-relative) | 40 B | 40,960 B | 2,621,400 B |

The index pool is 25% smaller than the pointer pool and 10% smaller than the
self-relative pool. A 16-bit self-relative field would also give 36 bytes, but
it cannot reach across this pool (section 1.3).

In Jet, `#Layout(c) struct Node { text: [U8#32], left: U16, right: U16 }`
reports `$layout.size` 36 and alignment 2. With `left` and `right` typed
`distinct U16` the size is still 36 (probe `size2.jet`), so a distinct type
costs no bytes. A default-layout struct reports no size, and a `U16?` field
has no C layout. Rule R3 in section 6 would give `NodeIx?` one.

### 2.3 Lookup speed

Each lookup walks from the root to a random existing key. The medians below are
over the runs listed beside them.

| Pool | Lookups | `ptr` | `idx` | `rel` |
|---|---:|---:|---:|---:|
| 1,024 | 20 M | **304 ms** (222–326) | 440 ms (388–490) | 393 ms (299–422) |
| 65,535, session 1 | 5 M | 1,120 ms (735–1,166) | **751 ms** (626–958) | 884 ms (644–946) |
| 65,535, session 2 | 5 M | **667 ms** (557–987) | 911 ms (718–1,297) | 763 ms (731–776) |

At 1,024 nodes the whole pool sits in the core's caches. Pointers were fastest,
at about 15 ns per lookup against 22 ns for indices and 20 ns for offsets.
**[INFERENCE: the index's multiply by 36 and the offset's add sit on the
dependent load chain, which matters when every load hits cache.]**

At 65,535 nodes the pools are 2.4 to 3.1 MB. The ranking flipped between two
sessions. Session 1 used zero-padded names (`node-%08d`), and session 2 the
final `node-%d`. Under this load, no encoding has a lookup advantage that can
be claimed from these runs.

### 2.4 Copying the whole pool

A copy models a snapshot, a save, or a send of the pool. Indices and offsets
copy with one `memcpy`. Pointers need a second pass that shifts every non-null
link to the new base.

| Pool | Copies | `ptr` + fixup | `idx` | `rel` |
|---|---:|---:|---:|---:|
| 1,024 | 100,000 | 166 ms (1.66 µs each) | 56 ms (0.56 µs) | 61 ms (0.61 µs) |
| 65,535 | 2,000 | 489 ms (245 µs each) | 89 ms (45 µs) | 97 ms (49 µs) |

The pointer copy was 3.0 times slower than the index copy at 1,024 nodes and
5.5 times slower at 65,535. Within a session the copy timings varied under 1.25
times, far less than the lookups.

### 2.5 A pool built into the program

`reloc.c` (appendix C) declares a constant 1,024-node pool, linked either by
pointers or by indices. It was built as a position-independent executable and
read with `readelf`, `size` and `objdump`.

| Measure | Pointer pool | Index pool |
|---|---:|---:|
| `R_X86_64_RELATIVE` relocations | 2,051 | 3 |
| `.rela.dyn` size | 49,344 B | 192 B |
| Section holding the pool | `.data.rel.ro`, 49,152 B | `.rodata`, 36,896 B |
| Binary size | 114,352 B | 52,832 B |

The three relocations in the index binary are the C runtime's `.init_array`,
`.fini_array` and `.data` entries, not the pool. The loader must patch every
pointer link before the program starts. **[INFERENCE: those patched pages
become private copies in each process. The index pool maps straight from the
file and can be shared.]**

The same holds for any file read with `mmap`. Indices and self-relative offsets
are valid wherever the bytes land, and pointers are not.

### 2.6 Jet against C

The Jet program uses a growable `[Node]` built with `push`. Each hop converts a
`U16` link to `Int` and indexes the list, and `cmp32` takes two `[U8#32]` by
value.

| Measure | C, word compare | C, byte compare | Jet |
|---|---:|---:|---:|
| 2 M lookups, 1,024 nodes | 33–89 ms | 103–152 ms | 487–1,047 ms |
| 200 k lookups, 65,535 nodes | 20 ms | — | 391 ms |
| One pool copy, 1,024 nodes | 0.56 µs (`memcpy`) | — | 6.1–8.2 µs |

Against the byte-compare C loop in the same session, Jet's lookups were 4 to 8
times slower. A Jet copy is `dup := pool.copy()` followed by one write, 11 to 15
times a C `memcpy`. A copy that is never written takes no time, so the compiler
elides it.

**[INFERENCE: the gap was not split by cause. Candidates are the per-hop list
bounds check, the 32-byte arrays passed by value to `cmp32`, and the `Int`
conversions.]**

Memory was probed separately (`memZ.jet`, `memA.jet`, `memB.jet`). Each pushes
65,535 nodes into a list and reports peak resident memory with GNU `time`.

| Probe | Peak RSS |
|---|---:|
| Baseline, no nodes | 4,628–4,892 KB |
| Default layout | 12,124 KB |
| `#Layout(c)` | 11,800–12,568 KB |

These are three runs of each probe. The nodes add about 7.4 MiB at peak, about
116 bytes per 36-byte node. The list doubles from 4, allocating the new buffer
before freeing the old (`Compiler/JetRuntime/Source/Collections.jet:186-202`).
At a 36-byte stride the last step holds both buffers, at most 54 bytes per node.
If no freed buffer were reused, all of them together would hold 72 bytes per
node. **[INFERENCE: the rest may come from the element stride being larger than
36, since `[T#N]` is represented as `Vec<T>` (spec.md:717-719). The cause was
not isolated.]** Work to store `[T#N]` inline is in flight on another card.

Whole-process RSS of the full benchmarks is not used. The C binary holds all
three encodings' pools at once, so its RSS does not isolate one encoding.

### 2.7 Caveats

- The machine ran other agents' builds throughout. Load averages of 39–52 on 32
  hardware threads mean every timing shares cores.
- Lookup timings varied up to 1.8 times between runs of one binary. Copy timings
  varied under 1.25 times.
- One machine, one compiler per language, no hardware counters.
- The Jet and C programs share the algorithm and the names, but not the random
  order. Jet's pool is a growable list, not a fixed array.

## 3. Costs

### 3.1 Range limits

A self-relative offset's reach is in bytes and depends on where the holder sits.
Making a struct bigger shrinks the reach in nodes, and a link that does not fit
fails at the moment it is written. Jai's 32-bit form and the ratified `Rel<T,
I32>` make this rare. The 16-bit form fails on the owner's pool.

An index's reach is in elements and does not depend on element size. A 16-bit
index covers any pool up to 65,536 elements, or 65,535 with a reserved null.

### 3.2 The add on every follow

A self-relative follow adds the offset to the field's own address. An index
follow multiplies by the element size and adds the base, and the base must be
in a register. Section 2.3 measured the combined effect at about 7 ns per
lookup for a cache-resident pool.

### 3.3 Copying a holder

Copying one node out of its pool breaks a self-relative link. In
`n := pool[3]`, the copy's offsets still measure from the old address, so they
point at the wrong place. A safe design must forbid following a self-relative
link outside its region or adjust it on every copy. The ratified `Rel<T>` does
the former: it is followed through `mem.Image`, and raw `get()` needs `#Unsafe`.

Copying one node with index links keeps them valid, as long as they are
followed in the same pool or an exact copy of it.

### 3.4 Null

Zero is free for a self-relative link, because a field never points at itself.
An index must reserve a value. The C benchmark reserves `0xFFFF`, so a 16-bit
pool holds 65,535 nodes. Zig reserves `maxInt`, and rustc reserves the top 256
values so that an optional index needs no extra tag.

### 3.5 Escape and aliasing

A self-relative link means something only at its own address. It cannot be held
in a local and followed after its holder moves. It must be turned into a normal
reference first.

An index means something only with its pool. Following it in the wrong pool of
the same element type reads the wrong node. If a pool can free and reuse slots,
an old index reads whatever now lives there. Generational handles (floooh,
slotmap, Jet's `Pool<T>`) detect reuse at the cost of a generation compare and
the bits to store it. A fixed pool that never frees has no reuse to detect.

## 4. Jet surfaces that already exist

- **`Pool<T>` and `ID<T>`** (spec.md:1441-1462): a generational arena. `add`
  returns an ID, `pool[id]` reads and writes, and a stale ID panics. The ratified
  text spells the type `Id<T>` (D-POOLID-API1, syntax-decisions.md:2786-2794);
  spec.md spells it `ID<T>`. The compiler accepts both spellings. A probe with
  `Pool<Node>.new()`, `.add`, `world[a].right = b` and `ids()` runs.
- **D-GAME-ENTITY1=A**: an entity is a slot index plus a generation, the same
  scheme as `Pool<T>`.
- **D-JAI-LAYOUT1=A** (card `c09uai16`): `packed`, `overlay`, and `Rel<T,
  I8|I16|I32|I64>` with zero as none. A `Rel` is followed through `mem.Image`
  with `root<T>()`, `follow(rel)` and a fallible `link(from, to)`. Raw
  `rel.get()` needs `#Unsafe`. Not built yet.
- **D-MEMREF1 and D-LINK-ITEM1=A** (syntax-decisions.md:2942-2952): `@` links,
  and `[@Node]` for kept links that point at objects. Not built yet.
- **D-RANGETYPE1 and D-TYPE2-REFINE1=A** (syntax-decisions.md:1243-1252,
  spec.md:730-734): `distinct Int(lo..hi)` values are proven in range, and "a
  proven `distinct Int(lo..hi)` or sized-integer interval may index without a
  runtime bounds check when `lo >= 0` and `hi < N`."
- **D-ARRAY-INIT1=A**: `[T#N].filled(v)` and `[T#N].from_fn(make)`. Ratified,
  not built.
- **`#Layout(c)`** (spec.md:709-713) allows `[T#N]` fields.

## 5. What Jet does today (probes)

Each probe below is a few lines, run with `jet check` or `jet run`. The
deciding line is quoted with its result, and the file name is a label.

### 5.1 Range types as indices

`NodeIx :: distinct Int(0..3)` works. Ranges are inclusive.
`NodeIx.from_int(1)` is total for a literal and fallible for a run-time value,
so a run-time conversion reads `NodeIx.from_int(k) ?? NodeIx.from_int(0)`.

### 5.2 Proofs that already hold

A function declared `-[!Panic]>` may not stop. In such a function,
`pool[at.raw()]` on a `[T#4]` is accepted when `at: NodeIx` is:

- a parameter;
- a local copy, `at := start`;
- a reassigned local, `at = other`;
- unwrapped from an optional with `??`, as in `at = next ?? return None`
  (`find.jet`, a complete tree walk that runs).

A sized integer proves itself too. `pool[i]` with `i: U8` on a `[Int#256]` is
accepted under `-[!Panic]>` (`u8ix.jet`). A `distinct U16` field takes 2 bytes
(section 2.2). Erasure, in other words, already works for the carrier and the
interval. What is missing is a range on a sized carrier.

### 5.3 Gaps

- **E0137 on a sized carrier.** `NodeIx :: distinct U16(0..1023)` reports
  "Inline value ranges apply to `Int`, not `U16`". Its fix text says "declare a
  named distinct type for another carrier", which this already is. The
  diagnostic contradicts itself (`u16r.jet`).
- **E0749 through a match payload.** In `if maybe == { .Val(at) ->
  pool[at.raw()] … }`, the proof is lost: "`from_option` can stop, but Panic is
  denied here" (`ix5.jet:10`). The `??` form of the same code passes.
- **Compile-time list constants crash.** `LEFT :: comptime { [Int#4]{1, -1, -1,
  -1} }` stops the backend with "ir.build unlowered … list constant of [Int]"
  (`ct2.jet`). A constant pool of `Node` fails the same way (`ctpool.jet`). So
  a pool built at compile time cannot reach the binary today.
- **`[T#N].filled` is not built**: `[U8#32].filled(0)` reports E0003.

## 6. An erasure design

The design adds no new sigil and no new kind of type. It extends `distinct`
range types, which already erase, and the `#` slot of `[T#N]`.

### 6.1 Rules

- **R1. Erasure.** A `distinct` value is its carrier at run time. `NodeIx ::
  distinct U16(0..1023)` is a bare 2-byte `U16`. Its range, and the array it
  indexes, exist only in the checker.
- **R2. Sized carriers.** `distinct U8|U16|U32|I8|I16|I32(lo..hi)` is allowed
  when the interval fits the carrier. An interval that does not fit is E0135.
  This lifts today's E0137.
- **R3. Null in a spare value.** When the interval leaves carrier values unused,
  `NodeIx?` stores `None` in one of them and stays 2 bytes. A full-range carrier
  needs a separate tag. The way out is to shrink the range by one, as Zig and
  rustc do.
- **R4. Proof from the type.** Indexing a `[T#N]` with a value whose type
  interval lies inside `0..N−1` never checks and never stops. This holds
  wherever the value was bound: parameter, local, field, element or match
  payload. It closes the E0749 gap in section 5.3.
- **R5. Index-typed arrays.** `[Node#NodeIx]` has one slot per `NodeIx` value,
  so `lo` must be 0 and the length is `hi + 1`. Only a `NodeIx` indexes it. An
  `Int` index is a type error whose fix names `NodeIx.from_int`.
- **R6. Sources.** A run-time `from_int` is fallible. Arithmetic on a `NodeIx`
  gives an `Int`. Returning to `NodeIx` goes through `from_int`, or through
  proposed `wrap` (modulo the range) and `clamp` constructors that cannot fail.

### 6.2 The owner's example under the design

Proposed spelling. The `U16` carrier and the `[Node#NodeIx]` array are new; the
rest parses today.

```jet
NodeIx :: distinct U16(0..1023)        // R2: 2 bytes at run time

#Layout(c)
struct Node {
    text: [U8#32]
    left: NodeIx?                      // R3: None is 65535, still 2 bytes
    right: NodeIx?
}

fn find(nodes: [Node#NodeIx], key: [U8#32]) -[!Panic]> NodeIx? {
    at := NodeIx.from_int(0)
    loop {
        c :: cmp32(key, nodes[at].text)     // R4, R5: no check, cannot stop
        if c == 0 -> return at
        next :: if c < 0 -> nodes[at].left else -> nodes[at].right
        at = next ?? return None
    }
}
```

`Node` is 36 bytes and the pool 36,864, the same as the C `idx` layout. Each
follow is the C `idx` follow: base plus index times 36, with no compare.

The same walk in today's spelling, with an `Int` carrier, `[Node#4]` and
`.raw()`, passes `-[!Panic]>` and runs (`find.jet`).

### 6.3 Bounds-proof rules, stated plainly

1. A pool whose capacity equals the index range needs no check. That is
   `[Node#NodeIx]` (R5), and also `[T#256]` with a `U8` index today.
2. An index taken from such a pool's own type is always in range, wherever it
   travels (R4).
3. A growable pool cannot be bounded by a type. `[Node]` indexed by `NodeIx`
   keeps a run-time check; `Pool<T>`/`ID<T>` keeps its generation check.
4. The card's other case, indices that only ever come from the pool, is rule 2
   for a fixed pool. For a growable pool it would need a brand per pool value,
   which section 6.4 declines; `Pool<T>` covers it with a generation check.
5. An index computed by arithmetic is an `Int` until it passes back through
   `from_int`, which is where the one check happens.

### 6.4 Restrictions and their way out

- **Pool identity is per type, not per pool value.** Two `[Node#NodeIx]` pools
  accept each other's indices. This follows `typed_index_collections`. It is
  memory-safe: the index is in range for both. The way out is a second distinct
  type, `OtherIx :: distinct U16(0..1023)`, which costs one line. A brand per
  pool value would put a brand parameter on every function that touches a pool,
  so it is not proposed.
- **`lo` must be 0 for an index-typed array.** The way out is to number from 0
  and add `lo` where the number is shown, one addition at that point. A range
  not starting at 0 still indexes a plain `[T#N]` through `raw() − lo` and
  `from_int`, one subtraction and one checked conversion.
- **Arithmetic leaves the type** (R6). This is a soundness rule: without it,
  `at + 1` could leave the pool.

### 6.5 What stays as it is

- Growable pools keep `Pool<T>`/`ID<T>`. Capacity-bounded lists keep run-time
  checks.
- `Rel<T, I32>` stays the tool for position-independent images through
  `mem.Image`. Indices do not replace it: an image may hold many regions with no
  shared base.

## 7. Mapping onto the array package

- **Typed index (this report)**: R1–R6 go into the array-programming ballot as
  its index rule. `[Node#NodeIx]` uses the `#` length slot that the open
  D-ARRAY-KEYED1 ballot proposes for enums, as in `[V2#Direction]`. A range
  type takes the place of the enum.
- **Compile-time pools**: once compile-time list constants lower (section 5.3),
  a pool built in a `comptime` block can be emitted as read-only data. With
  index links it needs no load-time relocation, as `reloc.c` shows for C.
- **Why indices for pools**: the same copy cost as self-relative offsets, no
  measured lookup penalty at scale, a free choice of width, and no
  copied-holder hazard.
- **Why `Rel` for images**: it needs no base, so it suits files and shared
  memory that mix many regions.

## 8. Defects found

These were found while probing and are reported, not fixed here.

1. E0137's fix text tells the user to do what they already did (section 5.3).
2. A range proof is lost through a match payload, E0749 (section 5.3).
3. Compile-time list constants crash the backend with "ir.build unlowered"
   (section 5.3).
4. One type has two spellings: spec.md writes `ID<T>`, D-POOLID-API1 writes
   `Id<T>`, and the compiler accepts both (section 4).
5. A Jet list of 36-byte nodes peaks near 116 bytes per node (section 2.6).

## Sources

- Jet: `Docs/spec/spec.md`, `Docs/spec/syntax-decisions.md`,
  `Docs/audits/jai-parity-2026-10-16.md`, and
  `Compiler/JetRuntime/Source/Collections.jet` at dev `e23bd7f503`; Tower
  decision D-JAI-LAYOUT1.
- Andre Weissflog, "Handles are the better pointers", floooh.github.io,
  2018-06-17, updated 2018-11-28.
- slotmap documentation, docs.rs/slotmap.
- typed_index_collections documentation, docs.rs/typed_index_collections.
- Zig, `lib/std/zig/Ast.zig` and `src/InternPool.zig`, github.com/ziglang/zig
  master.
- rustc, `compiler/rustc_index_macros/src/newtype.rs` and `lib.rs`,
  github.com/rust-lang/rust master.
- Cap'n Proto, "Encoding", capnproto.org/encoding.html.
- FlatBuffers, "Internals", flatbuffers.dev/internals/.
- Odin, "Overview", odin-lang.org/docs/overview/.

## Appendix A. `pool2.c`

Build: `gcc -O2 [-DN=65535] [-DLOOKUPS=…] [-DCOPIES=…] [-DBYTECMP] -o pool
pool2.c`. Run: `./pool ptr|idx|rel`.

```c
// #6204 benchmark v2: a binary search tree in a fixed pool, linked three ways.
// Run one variant per process: ./pool2 ptr|idx|rel. Same inline key compare for all.
// ptr: 64-bit pointers. idx: uint16 indices. rel: int32 self-relative byte offsets.
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>

#ifndef N
#define N 1024
#endif
#ifndef LOOKUPS
#define LOOKUPS 20000000
#endif
#ifndef COPIES
#define COPIES 100000
#endif
#define NONE 0xFFFF

typedef struct NodeP { char text[32]; struct NodeP *left, *right; } NodeP;
typedef struct NodeI { char text[32]; uint16_t left, right; } NodeI;
typedef struct NodeR { char text[32]; int32_t left, right; } NodeR;

static NodeP pool_p[N], copy_p[N];
static NodeI pool_i[N], copy_i[N];
static NodeR pool_r[N], copy_r[N];
static int order[N];
#define KEYS (N > 4096 ? 65536 : 4096)
static char keys[KEYS][32];
static volatile long sink;

static double now(void) {
    struct timespec t;
    clock_gettime(CLOCK_MONOTONIC, &t);
    return t.tv_sec * 1e3 + t.tv_nsec / 1e6;
}

static uint64_t rng = 88172645463325252ull;
static uint64_t next(void) { rng ^= rng << 13; rng ^= rng >> 7; rng ^= rng << 17; return rng; }

static void name(char *text, int k) { memset(text, 0, 32); snprintf(text, 32, "node-%d", k); }

// Lexicographic compare of two 32-byte texts: 8 bytes at a time, or byte by
// byte with -DBYTECMP (the loop the Jet side writes).
static inline int cmp32(const char *a, const char *b) {
#ifdef BYTECMP
    for (int i = 0; i < 32; i++) {
        if (a[i] != b[i]) return (unsigned char)a[i] < (unsigned char)b[i] ? -1 : 1;
    }
    return 0;
#else
    for (int w = 0; w < 4; w++) {
        uint64_t x, y;
        memcpy(&x, a + 8 * w, 8); memcpy(&y, b + 8 * w, 8);
        if (x != y) return __builtin_bswap64(x) < __builtin_bswap64(y) ? -1 : 1;
    }
    return 0;
#endif
}

static NodeR *rel_get(int32_t *field) { return *field ? (NodeR *)((char *)field + *field) : NULL; }
static void rel_set(int32_t *field, NodeR *to) { *field = to ? (int32_t)((char *)to - (char *)field) : 0; }

int main(int argc, char **argv) {
    char v = argc > 1 ? argv[1][0] : 'i';
    for (int k = 0; k < N; k++) order[k] = k;
    for (int k = N - 1; k > 0; k--) { int j = next() % (k + 1); int t = order[k]; order[k] = order[j]; order[j] = t; }
    for (int k = 0; k < KEYS; k++) name(keys[k], next() % N);
    for (int s = 0; s < N; s++) {
        int k = order[s];
        name(pool_p[s].text, k); pool_p[s].left = pool_p[s].right = NULL;
        name(pool_i[s].text, k); pool_i[s].left = pool_i[s].right = NONE;
        name(pool_r[s].text, k); pool_r[s].left = pool_r[s].right = 0;
        if (s == 0) continue;
        NodeP *p = &pool_p[0];
        for (;;) { NodeP **slot = cmp32(pool_p[s].text, p->text) < 0 ? &p->left : &p->right; if (!*slot) { *slot = &pool_p[s]; break; } p = *slot; }
        int i = 0;
        for (;;) { uint16_t *slot = cmp32(pool_i[s].text, pool_i[i].text) < 0 ? &pool_i[i].left : &pool_i[i].right; if (*slot == NONE) { *slot = (uint16_t)s; break; } i = *slot; }
        NodeR *r = &pool_r[0];
        for (;;) { int32_t *slot = cmp32(pool_r[s].text, r->text) < 0 ? &r->left : &r->right; if (!*slot) { rel_set(slot, &pool_r[s]); break; } r = rel_get(slot); }
    }
    double t0, t1, t2;
    long found = 0;
    t0 = now();
    if (v == 'p') {
        for (long q = 0; q < LOOKUPS; q++) {
            const char *key = keys[q & (KEYS - 1)];
            NodeP *p = &pool_p[0];
            while (p) { int c = cmp32(key, p->text); if (c == 0) { found++; break; } p = c < 0 ? p->left : p->right; }
        }
    } else if (v == 'i') {
        for (long q = 0; q < LOOKUPS; q++) {
            const char *key = keys[q & (KEYS - 1)];
            uint32_t i = 0;
            while (i != NONE) { int c = cmp32(key, pool_i[i].text); if (c == 0) { found++; break; } i = c < 0 ? pool_i[i].left : pool_i[i].right; }
        }
    } else {
        for (long q = 0; q < LOOKUPS; q++) {
            const char *key = keys[q & (KEYS - 1)];
            NodeR *r = &pool_r[0];
            while (r) { int c = cmp32(key, r->text); if (c == 0) { found++; break; } r = rel_get(c < 0 ? &r->left : &r->right); }
        }
    }
    t1 = now(); sink = found;
    // Copy the whole pool elsewhere (snapshot, save, send). Pointers need a fixup pass.
    for (int c = 0; c < COPIES; c++) {
        if (v == 'p') {
            memcpy(copy_p, pool_p, sizeof(pool_p));
            intptr_t shift = (char *)copy_p - (char *)pool_p;
            for (int k = 0; k < N; k++) {
                if (copy_p[k].left) copy_p[k].left = (NodeP *)((char *)copy_p[k].left + shift);
                if (copy_p[k].right) copy_p[k].right = (NodeP *)((char *)copy_p[k].right + shift);
            }
            sink += (long)copy_p[c % N].left;
        } else if (v == 'i') {
            memcpy(copy_i, pool_i, sizeof(pool_i)); sink += copy_i[c % N].left;
        } else {
            memcpy(copy_r, pool_r, sizeof(pool_r)); sink += copy_r[c % N].left;
        }
    }
    t2 = now();
    size_t node = v == 'p' ? sizeof(NodeP) : v == 'i' ? sizeof(NodeI) : sizeof(NodeR);
    printf("%c N=%d node %zu B pool %zu B lookups %.0f ms copies %.0f ms found %ld\n", v, N, node, node * N, t1 - t0, t2 - t1, found);
    return 0;
}
```

## Appendix B. `pool.jet`

Build: `jet build --release --allow=Time pool.jet`. The 65,535-node variant
changes `n`, `lookups` and the key count.

```jet
// #6204 benchmark, Jet side: the owner's Node pool linked by U16 indices.
use time :: core.time

struct Node {
    text: [U8#32]
    left: U16
    right: U16
}

fn name_of(k: Int) -> [U8#32] {
    out := [U8#32]{0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0}
    digits :: "node-{k}"
    i := 0
    loop b in digits.bytes() {
        if i < 32 {
            out[i] = b
        }
        i += 1
    }
    out
}

fn cmp32(a: [U8#32], b: [U8#32]) -> Int {
    i := 0
    loop i < 32 {
        if a[i] != b[i] {
            return if a[i] < b[i] -> -1 else -> 1
        }
        i += 1
    }
    0
}

fn run() {
    n :: 1024
    lookups :: 2000000
    none :: U16{65535}
    seed := 88172645463325252
    order := [Int]{}
    k := 0
    loop k < n {
        &order.push(k)
        k += 1
    }
    k = n - 1
    loop k > 0 {
        seed = (seed * 6364136223846793005 + 1442695040888963407) % 9223372036854775807
        j :: (if seed < 0 -> -seed else -> seed) % (k + 1)
        t :: order[k]
        order[k] = order[j]
        order[j] = t
        k -= 1
    }
    pool := [Node]{}
    s := 0
    loop s < n {
        &pool.push(Node{text: name_of(order[s]), left: none, right: none})
        if s > 0 {
            i := 0
            placed := false
            loop !placed {
                if cmp32(pool[s].text, pool[i].text) < 0 {
                    if pool[i].left == none {
                        pool[i].left = U16{s}
                        placed = true
                    } else {
                        i = Int{pool[i].left}
                    }
                } else {
                    if pool[i].right == none {
                        pool[i].right = U16{s}
                        placed = true
                    } else {
                        i = Int{pool[i].right}
                    }
                }
            }
        }
        s += 1
    }
    keys := [[U8#32]]{}
    k = 0
    loop k < 4096 {
        seed = (seed * 6364136223846793005 + 1442695040888963407) % 9223372036854775807
        &keys.push(name_of((if seed < 0 -> -seed else -> seed) % n))
        k += 1
    }
    t0 :: time.instant()
    found := 0
    q := 0
    loop q < lookups {
        key :: keys[q % 4096]
        i := 0
        searching := true
        loop searching {
            c :: cmp32(key, pool[i].text)
            if c == 0 {
                found += 1
                searching = false
            } else {
                next :: if c < 0 -> pool[i].left else -> pool[i].right
                if next == none {
                    searching = false
                } else {
                    i = Int{next}
                }
            }
        }
        q += 1
    }
    ms :: t0.elapsed_millis()
    print("jet idx N={n} lookups {lookups}: {ms} ms found {found}")
    t1 :: time.instant()
    total := 0
    c := 0
    loop c < 10000 {
        dup := pool.copy()
        dup[c % n].left = U16{c % 7}
        total += Int{dup[(c * 7) % n].left}
        c += 1
    }
    ms2 :: t1.elapsed_millis()
    print("jet copies 10000: {ms2} ms total {total}")
}
```

## Appendix C. `reloc.c`

Build: `gcc -O2 -fPIE -pie [-DPTR] -o reloc reloc.c`, then `readelf -r`.

```c
// #6204: a build-time (constant) 1024-node pool linked by pointers vs by u16
// indices. Build with -DPTR or without, as a PIE, then count load-time
// relocations with readelf -r.
#include <stdint.h>
#include <stdio.h>

#define R4(M, i) M(i), M(i + 1), M(i + 2), M(i + 3)
#define R16(M, i) R4(M, i), R4(M, i + 4), R4(M, i + 8), R4(M, i + 12)
#define R64(M, i) R16(M, i), R16(M, i + 16), R16(M, i + 32), R16(M, i + 48)
#define R256(M, i) R64(M, i), R64(M, i + 64), R64(M, i + 128), R64(M, i + 192)
#define R1024(M, i) R256(M, i), R256(M, i + 256), R256(M, i + 512), R256(M, i + 768)

#ifdef PTR
typedef struct NodeP { char text[32]; const struct NodeP *left, *right; } NodeP;
extern const NodeP pool[1024];
#define MK(i) { "n", &pool[(2 * (i) + 1) & 1023], &pool[(2 * (i) + 2) & 1023] }
const NodeP pool[1024] = { R1024(MK, 0) };
int main(int argc, char **argv) {
    (void)argv;
    const NodeP *p = &pool[0];
    for (int k = 0; k < 10 + argc; k++) p = p->left;
    printf("%d\n", (int)(p - pool));
    return 0;
}
#else
typedef struct NodeI { char text[32]; uint16_t left, right; } NodeI;
#define MK(i) { "n", (2 * (i) + 1) & 1023, (2 * (i) + 2) & 1023 }
const NodeI pool[1024] = { R1024(MK, 0) };
int main(int argc, char **argv) {
    (void)argv;
    unsigned i = 0;
    for (int k = 0; k < 10 + argc; k++) i = pool[i].left;
    printf("%u\n", i);
    return 0;
}
#endif
```
