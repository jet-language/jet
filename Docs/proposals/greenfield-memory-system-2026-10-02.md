# One owner: a greenfield memory system for Jet

Status: research concept, revision 8, 2026-10-02. This is a design for the
memory system Jet should have, written in Jet's general syntax. Every memory
construct here is a proposal, not current behavior. How to move Jet from
today's model to this one is a later discussion. Nothing here is ratified,
and no Tower card owns this yet.

## Design principles

1. **One rule at the core.** Every value has exactly one owner, and *a call
   changes only what its caller marked `&`*.
2. **Every capability has a beginner default and an expert form.** The default
   is implicit and safe. The expert form gives full control.
3. **One control ladder for everything.** The compiler *infers* each mechanism,
   *shows* it, lets you *pin* it by writing it, and under `#Strict` makes you
   write all of it.
4. **Merge, never remove.** A feature leaves only when another mechanism does
   its job completely. Otherwise it stays, simplified where possible.

## The core

Jet values behave like numbers: assigning one gives you your own. A function
reads what it receives unless the caller hands it over with `&`.

```
fn curve(scores: &[Int], points: Int) {
    loop i in scores.indexes() -> scores[i] += points
}

fn run() {
    scores :: [90, 85, 77]
    curved := scores
    curve(&curved, 5)
    print(scores)            // [90, 85, 77]
    print(curved)            // [95, 90, 82]
}
```

Values come in two kinds, and the type tells you which:

- **Owned values**, the default. They have one owner, and only that owner
  changes them, through `&`.
- **Shared values**, `Shared<T>` and `Atomic<T>`. They have many holders, any
  of whom may change them, one change at a time. You choose one when you want
  exactly that.

Reading a single line tells you three things:

1. **A call changes only what is marked `&` on its line,** plus anything
   those values hold through `&`, plus shared values it can reach.
2. **An owned value changes only through its owner.**
3. **Nothing you can name is ever freed under you.**

`jet explain` lists every shared value a function can reach, so the third
part of guarantee 1 is always inspectable.

## The control ladder

Every memory mechanism goes through the same four steps:

| Step | Who acts | Example |
|---|---|---|
| **Infer** | The compiler picks the cheapest safe mechanism | It compiles `curved := scores` as a copy, because `scores` is used again |
| **Show** | The editor and `jet explain memory` display the choice in Jet's own spelling | `curved := ~scores` appears faintly |
| **Pin** | You write the spelling, and it becomes a checked requirement | You write `~scores` or `^scores`, and the compiler holds you to it |
| **`#Strict`** | You must write every mechanism with a runtime cost | Unwritten copies, counts, unproven checks, vectorization fallbacks, and GPU barriers become errors that show the line to write |

```
#Strict
fn top(scores: [Int], n: Int) -> [Int] {
    sorted := scores
    &sorted.sort()
    return sorted.take(n)
}
```

```
error: #Strict code copies `scores` here (4,096 Int, 32 KiB)
 --> rank.jet:3:15
  |
3 |     sorted := scores
  |               ^^^^^^ `scores` belongs to the caller, so sorting it needs a copy
  |
  = fix: write `sorted := ~scores`, or have the caller hand it over: `top(^scores, 10)`
```

A `#Strict` function means exactly what it would mean without `#Strict`. To
convert a function, accept the spellings the editor shows and add `#Strict`.
Script code becomes systems code without a rewrite.

## Capability map

| Capability | Beginner default | Expert control | Under `#Strict` |
|---|---|---|---|
| Copy or hand off | Inferred | `~x`, `^x` | Every copy written |
| Change through a call | `&x` | Same | Same |
| Borrowed data | Plain fields, shared copy-on-write | `View<T>`; `&T` fields and returns; `from` | Sharing proven or written |
| Identity and graphs | `Pool<T>` + `Id<T>` | Removal, generation checks | Stale-id checks proven or written |
| Bulk lifetimes and allocators | Inferred | `Arena`; any container built in an allocator | Same |
| Many owners | `shared x` | `Shared.Weak<T>`, guards, `Atomic<T>` with orderings | Counts written |
| Dynamic object graphs | `#Policy(gc)` scope | Explicit collect points | Forbidden |
| Concurrency and async | `.parallel()`, task groups, `async` calls | Channels, atomics, lock-free code over `Pool` | Same |
| Resources | Closed at scope end | `defer`, explicit close | Same |
| Placement and layout | Inferred | `Pin<T>`, layout attributes, `try_` allocation | Allocation written |
| No-allocation and real-time code | Not needed | Authority at package, module, or function scope | Same |
| SIMD | Auto-vectorized, shown | Vector types (plain data), layout attributes | Fallback to scalar code written |
| Raw storage | Not needed | `Buffer<T>`: typed or bytes, possibly uninitialized | Init state proven or tracking written |
| FFI, memory maps, kernels | Not needed | `Buffer.adopt`, `Buffer.map`, `Buffer.at`; `*T` | Same |
| Hardware | Not needed | `Device<Regs>`, inline `asm` | Same |
| Untrusted facts | Not needed | `#Unsafe("reason")` | Same |
| GPU | `core.compute` tensors | Tile loops, `gpu.shared`, explicit barriers and streams | Barriers and layouts written |

## Capabilities

### Borrowed data

**Default:** write plain fields. When the source outlives the struct, the
compiler shares it copy-on-write instead of copying. If anyone later changes
either side, that side gets its own copy, so meaning never changes. The editor
shows what it did:

```
struct Token {
    text: String
    rest: String
}

fn scan(source: String) -> Token {
    Token{text: source.before(":"), rest: source.after(":")}
}                                           // shown: text, rest share source · no copy
```

**Expert:** `View<T>` is the type that guarantees no copy. `&T` stores
exclusive access in fields and returns. Both record where they borrow from;
that provenance is inferred, and `from` pins it at API boundaries:

```
struct Token {
    text: View<str>
    rest: View<str>
}

fn longer(left: String, right: String) -> View<str> from left | right {
    if left.len() >= right.len() -> return left.trim()
    return right.trim()
}

struct Cursor {
    buffer: &[U8]
    pos: Int
}

fn run() {
    data := [U8]{1, 2, 3}
    c := Cursor{buffer: &data, pos: 0}       // c borrows data; data can't be used until c ends
    &c.write(9)                               // changes data, through what c holds with &
    print(data)
}
```

A value that holds a `View` or `&T` can't outlive its source. It also can't
be put into anything with many holders or an open-ended lifetime: a `Pool`,
a `Shared`, module state, or a detached task. `&T` replaces today's separate
`ViewMut<T>`: there is one spelling for exclusive access everywhere.

### Identity and graphs

A `Pool` owns its objects, and an `Id` names one of them. Ids can form cycles
because they own nothing. When the pool goes, everything in it goes.

```
struct Person {
    name: String
    friends: [Id<Person>]
}

fn run() {
    people := Pool<Person>{}
    ada :: &people.add(Person{name: "Ada", friends: []})
    bob :: &people.add(Person{name: "Bob", friends: []})
    &people[ada].friends.push(bob)
    &people[bob].friends.push(ada)          // a cycle, and that's fine
    &people.remove(bob)
    print(people.has(bob))                  // false
    print(people[bob].name)                 // stops: "bob was removed from people"
}
```

A pool that never removes needs no generation checks. Under `#Strict`, every
stale-id check must be proven or written as `people.get(bob)`.

### Allocators and arenas

Allocators are not values you change. Allocating has no effect you can
observe on any value you can name, whether it comes from the heap or from an
`Arena`. An `Arena` exposes no size or count, only debugging statistics
through `jet explain`. That is why allocating from an arena needs no `&`, just
as allocating from the heap needs none.

Any owning container can be built in an allocator:

```
fn frame(world: World) {
    scratch :: Arena{}
    visible :: [Mesh].in(scratch)
    …
}                                           // everything allocated in scratch is freed here, at once
```

Whatever is built in an arena records it as its source, so it can't outlive
the arena. An arena belongs to one task unless its type is a concurrent arena.
A bump allocator over `Buffer<U8>` (see [Raw storage](#raw-storage)) is an
ordinary user-written allocator.

### Many owners

`shared x` makes a value with many holders whose changes happen one at a time.
It is the type for state that outlives any single scope.

```
fn serve(cache: Shared<[String:Page]>, url: String) {
    page :: fetch(url)
    cache.with((pages: &[String:Page]) -> { pages[url] = page })
}

fn run() {
    cache :: shared [String:Page]{}
    …
}
```

**Expert:**

- `Shared.Weak<T>` makes back edges and caches.
- Guards hold the lock across several steps.
- `Atomic<T>` gives lock-free cells with explicit memory orderings.

Changes to shared values are not marked with `&`, because `&` means exclusive
access. The type already says that anyone may change the value.

Strong cycles of `Shared` are rejected at compile time by type: a type whose
`Shared` handles can reach the same type is refused. The cost is that a
`Shared<Node>` structure is rejected even when it would be acyclic at run
time. The fixes are owned children (plain values), `Weak` back edges, a
`Pool`, or a `#Policy(gc)` scope. Because of that rejection, counting alone
frees all memory outside a GC scope.

### Dynamic object graphs: `#Policy(gc)`

Some programs need arbitrary shared, cyclic graphs: interpreters, symbolic AI,
graph databases, compiler IR, and code ported from Python or JavaScript.
Inside a `#Policy(gc)` scope, `Shared` handles may form cycles, and a tracing
collector reclaims them.

A handle made inside the scope never leaves it except as a deep copy, so the
collector always sees every root and never scans the rest of the program. It
runs when the scope allocates or calls `gc.collect()`. The editor marks every
collected scope. `#Strict` and package authority can forbid collection.

### Concurrency and async

```
fn brighten(image: &Image) {
    loop row in &image.rows.parallel() {
        loop x in row.indexes() -> row[x].light += 10
    }
}
```

`&image.rows.parallel()` is an ordinary `&`-marked call that returns `&`
pieces of `image.rows`, one per iteration. Inside the loop, `image.rows` can't
be named, and each loop variable is the only access to its piece. That rule
also forbids `push` on a list while a loop is changing it. Iterations that
could change the same place are a compile error, not a race.

- Task groups borrow from their scope.
- Detached tasks take owned values (`^x`) or `Shared` handles.
- Channels move values between tasks.
- An `async` suspension point counts as a call: values held across it follow
  the same task rules.
- The state machines the compiler generates for async code are pinned
  internally, so users never write `Pin` for async.

**Lock-free code.** Keep nodes in a `Pool` and swap `Atomic<Id<T>>` values.
The `Id` generation check also guards against the ABA problem, and a removed
node can never be reached.

Every data race is ruled out at compile time.

### Resources, placement, and layout

Resources such as files, sockets, and locks close at the end of their scope,
in reverse order, on every path. `defer` and explicit `close` are the expert
forms. Plain memory is freed after its last use.

The compiler chooses where values live and how they are laid out. Experts
control that with:

- **`Pin<T>`** for a stable address. FFI callbacks and DMA need one, and
  `Pool`, `Arena`, and `Buffer` storage is always stable.
- **Layout attributes,** such as C, packed, and columnar.
- **`try_` allocation** to handle running out of memory.
- **Authority at package, module, or function scope.** It can forbid heap
  allocation, counting, or collection, which is how real-time and
  no-allocation code is written.

SIMD vector types are plain data. Auto-vectorization follows the ladder: it is
inferred and shown, and under `#Strict` a scalar fallback must be written.

### Raw storage

`Buffer<T>` is the one raw-storage type. It is a block of possibly
uninitialized slots of type `T`.

**Raw bytes are `Buffer<U8>`, and an address is an `Int` offset.**

- Pointer arithmetic and tagging are ordinary integer math.
- Every access is checked against the buffer's bounds unless the compiler
  proves it in range.
- Only plain-data types can be read from bytes.
- A bug corrupts bytes inside that buffer and nothing else.

```
struct Header {
    size: U32
    next: U32
}

fn bump(heap: &Buffer<U8>, top: &Int, size: Int) -> Int? {
    start :: align_up(top, 8)
    if size > heap.len() - start -> return None
    top = start + size
    return start
}

fn run() {
    heap := Buffer<U8>.zeroed(1_048_576)
    top := 0
    p :: bump(&heap, &top, 64) ?? panic("out of memory")
    &heap.write(p, Header{size: 64, next: 0})
    print(heap.read<Header>(p).size)        // 64
}
```

**Typed buffers hold any type,** which is enough for containers such as a
small vector, a ring buffer of `String`s, or a B-tree. Slots change only
through the `Buffer` API. The compiler tracks which slots are initialized
where it can, and otherwise keeps a runtime map, which the editor shows. Under
`#Strict` you either prove the initialized slots or write
`Buffer<T>.tracked(n)` to keep the map. This generalizes today's
`[U8#4]{uninit}`.

**Memory from outside Jet** comes in through `Buffer`:

- `Buffer.map(file)` maps a file. The buffer is a resource, and its bytes may
  change under you, which is safe because only plain data is read from bytes.
- `Buffer.adopt(ptr, len, release)` takes ownership of foreign memory, such as
  a `malloc` result. It runs inside `#Unsafe`. Adoption is the one trusted
  step, and `release` runs when the buffer closes.
- `Buffer<U8>.at(address, len)` gives a physical region such as a page table
  or an MMIO window. It runs inside `#Unsafe`.

**`*T` raw pointers** stay as they are today, inside `#Unsafe`. They serve
FFI signatures and the rare code that needs a thin, unchecked pointer.

### Hardware and assembly

`Device<Regs>` is a typed register block that you own. The compiler never
removes, merges, or reorders its reads and writes. Only its address needs
trust:

```
fn send(uart: &Device<UartRegs>, byte: U8) {
    loop uart.status.tx_full {}
    uart.data = byte
}

fn run() {
    uart := #Unsafe("RP2040 datasheet §4.2: UART0 registers start at 0x4000_C000") {
        Device<UartRegs>.at(0x4000_C000)
    }
    send(&uart, U8{65})
}
```

Interrupt handlers share state with the main program through module-level
`Atomic` or `Shared` values. An inline `asm` block declares the buffers it
touches. When the compiler can prove every memory operand stays inside them,
the block needs no `#Unsafe`.

`#Unsafe("reason")` admits only operations whose safety can't be checked:
foreign calls, `*T` access, unproven `asm`, adopting foreign memory, and
physical addresses. Everything else inside the block stays checked. `jet
explain unsafe` lists every such block in a build, including those in
dependencies.

### GPU

`core.compute` remains the default for tensors, device placement, streams,
transfers, and `#Kernel` element-wise functions. Custom kernels follow the
same ownership rule:

```
fn matmul(a: Matrix<F16>, b: Matrix<F16>, c: &Matrix<F32>) {
    loop (at, out) in &c.gpu_tiles(128, 128) {
        acc := Matrix<F32>.zeros(128, 128)
        loop k in 0..<a.cols() / 32 {
            x :: gpu.shared(a.tile(at.row, k, size: (128, 32)))
            y :: gpu.shared(b.tile(k, at.col, size: (32, 128)))
            &acc.multiply_add(x, y)
        }
        out = acc
    }
}
```

- Each block owns a different `out` tile, so blocks can't race.
- `gpu.shared` may appear only at the top level of a tile loop, where every
  thread in the block runs it.
- The compiler places a barrier between the cooperative write of a shared
  tile and the reads that follow, and another before the tile is reused. It
  also overlaps the next load with the current multiply.
- The compiler splits `acc` across the block's threads in a register layout.
  Under `#Strict`, that layout and every barrier are written out, and the
  compiler checks them.

## Compared with Jet today

| Jet today | This design | Why |
|---|---|---|
| Value semantics, unmarked read parameters, `&T` and `&x` | **Kept** | This is the core rule |
| `^` and `~`, written where the checker requires them | **Changed:** inferred and shown, written to pin, required under `#Strict` | Beginners write less, and experts lose nothing |
| `View<T>` fields and returns, `from` | **Kept** as the no-copy guarantee; plain fields share copy-on-write by default | Beginners use plain fields; experts keep the zero-copy guarantee |
| `ViewMut<T>` | **Merged** into `&T`, now allowed in fields and returns | One spelling for exclusive access |
| `Pin<T>` | **Kept**; async code is pinned internally | FFI and DMA need stable addresses |
| `Shared<T>`, `shared x`, guards, `Shared.Weak<T>` | **Kept**; changes stay unmarked; the cycle rule is by type | The one many-owner type |
| `Pool<T>` + `Id<T>` | **Kept**; also the basis for lock-free reclamation | Graphs and ABA-safe nodes |
| No arena or allocator parameter | **Added:** `Arena`, and containers built in any allocator | Games, compilers, request-scoped memory |
| `Atomic<T>` | **Kept**, with explicit orderings for experts | Lock-free code |
| `#Policy(gc)` | **Kept**, redefined as `Shared` with cycles allowed, a collector, and no handle escaping | One sharing mechanism, with GC as its fallback |
| `*T`, `*x`, and `p.*` inside `#Unsafe` | **Kept** | FFI needs thin pointers; checked access goes through `Buffer` and `View` |
| `[T#n]{uninit}` | **Generalized** into `Buffer<T>`, which also covers raw bytes, file maps, adopted foreign memory, and physical regions | One raw-storage type |
| Volatile memory access inside `#Unsafe` | **Kept**, with `Device<Regs>` added as a safe layer | Drivers need `#Unsafe` only for the address |
| `#Unsafe("reason")` | **Kept**, limited to operations that can't be checked | A smaller audit surface |
| Inline `asm` | **Kept**, with declared footprints | Proven blocks need no `#Unsafe` |
| `core.compute`, `#Kernel` | **Kept**, with tile and shared-memory ownership and barrier inference added | Custom kernels under the same rule |
| Layout attributes, `defer`, resource close, package authority | **Kept**; authority also works at module and function scope | Real-time and no-allocation code |
| Not present | **Added:** the inference and display ladder, `#Strict`, loops that change their source, `jet explain memory` | The improvements over today |

## Earlier revisions corrected

- **Revision 6 removed six capabilities without proof** that anything else
  covered them: stored views, `Shared`/`Weak`, `*T`, GC, `Pin`, and module
  state. Revision 7 restored all six.
- **Revision 7 had four soundness holes.** Revision 8 fixes them:
  - `&T` fields could escape their source.
  - An allocation exemption that could be observed.
  - GC handles could escape their scope.
  - A "safe `*T`" that duplicated `View`.
- **Revision 7 had two bolt-ons.** Revision 8 removes them:
  - Four raw-memory layers, now one `Buffer<T>`.
  - A module-effect arrow that broke higher-order calls.
- **Revision 8 fills seven capability gaps:** allocators for any container,
  async, lock-free reclamation, FFI ownership transfer, memory-mapped files,
  SIMD, and function-level no-allocation code.

## The safety claim

> **Jet programs have no undefined behavior, no dangling data, and no data
> races. A program can fail only in ways its source shows: a check that stops
> it, wrong bytes in its own `Buffer`, or an `#Unsafe` reason that was false.**

## Decisions to ballot

1. **The control ladder:** infer, show, pin, and `#Strict` for every memory
   mechanism, with `~` and `^` inferred by default.
2. **Plain fields share copy-on-write by default**, and `View<T>` guarantees
   no copy.
3. **`&T` everywhere** for exclusive access, absorbing `ViewMut<T>`. A value
   holding `&T` or `View` can't enter a store with many holders.
4. **Allocators are unobservable**, so `Arena` allocation needs no `&`, and any
   container can be built in an allocator.
5. **The `Shared` cycle rule is by type**, and `#Policy(gc)` is `Shared` with
   cycles allowed, a collector, and no handle escaping.
6. **`Buffer<T>` as the one raw-storage type**, covering bytes, uninitialized
   slots, file maps, adopted foreign memory, and physical regions.
7. **Authority at function scope** for no-allocation and real-time code.
8. **Loops that change their source**, and GPU tile ownership with inferred
   barriers.

## Revision history

- **Revisions 1–2:** a permission calculus with many mechanisms. The owner
  rejected it as complex.
- **Revisions 3–4:** values with new keywords, not in Jet syntax. Rejected.
- **Revisions 5–6:** Jet syntax and one rule. These converged on Jet's
  existing core while removing capabilities without justification.
- **Revision 7:** merge, never remove. It restored those capabilities and
  kept the ladder and `#Strict`.
- **Revision 8:** applies an owner-authorized Fable 5.1 review. It fixes
  soundness holes, fills capability gaps, and merges raw storage into
  `Buffer<T>`.
