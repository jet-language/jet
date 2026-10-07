# Jet-native backend design (D-EXEC1, card #4015)

This document is the contract for the Jet-written code generator that replaces
Cranelift for O0 and O1. It covers the typed low-level IR (LIR), the MIR ->
LIR lowering, the x86-64 System V generator (instruction selection, register
allocation, frames), relocations and images (static ELF, in-memory), the
runtime the generated code calls, and how tiered recompilation swaps
functions.

Sources: `Compiler/JetBackend/Source/LIR/` (LIR types, lint, printer),
`Compiler/JetBackend/Source/Lower/Lower.jet` (MIR -> LIR),
`Compiler/JetBackend/Source/X64/` (encoder, register allocator, instruction
selection), `Compiler/JetBackend/Source/Image/` (linker, freestanding image
runtime, ELF writer, in-process loader) and `Compiler/JetBackend/Source/OS/`
(the Jet OS layer). The package is listed in `Compiler/Bootstrap/sources.list`
after the Foundation MIR it reads.

### What Cranelift does today (the behaviour to match)

`crates/jet-jit/src/jit/runtime_host.rs` (`new_jit_module`) builds one
`JITModule` with `opt_level=speed`, `is_pic=true` and `hotswap(true)`, and
registers every runtime host function by symbol name on the `JITBuilder`.
`crates/jet-jit/src/jit/functions_compile.rs` translates each MIR function
straight to Cranelift IR (one `i64` per value, `i8` Bools, runtime calls by
imported symbol), and `backend.rs` (`CraneliftBackend::run`, `hot_swap`,
`restart`) defines the functions, calls `finalize_definitions` (which maps
code writable, copies it, then flips it to read+execute) and runs the entry.
Hot swap redefines a function in place; `hotswap(true)` routes every call
through an indirection so redefinition reaches existing callers. The Jet
backend keeps these observable contracts: same runtime symbols, same value
meaning at every call boundary, W^X code memory, and replaceable functions.

## LIR

LIR (`LIR/LIR.jet`) is the typed, target-independent form every Jet code
generator level reads. A `LIRModule` holds signatures, functions and
read-only data; each table's ID equals its index.

- Every `LIRValue` carries its `LIRLayout` (size, alignment and System V
  class `Integer | Pointer | Float | Memory`), its drop action
  (`Trivial`, `Runtime(symbol)` for compiled-runtime drop glue,
  `Function(id)` for glue generated into the module) and the checked Jet type
  it came from (`MIRTypeID?`).
- Blocks take parameters instead of phi nodes; the entry block's parameters
  are the function parameters, and no jump targets the entry.
- Instructions: `Const`, `Binary` (`Add Sub Mul Div Rem And Or Xor Shl Shr`,
  signed, `Shr` arithmetic), `Compare` (signed `Eq Ne Lt Le Gt Ge`), `Move`
  (ownership transfer; the source dies), `Copy` (trivially destructible values
  only), `Call` (signature ID; callee a module function or a runtime symbol),
  `Drop`, `Overflows`, `Load`, `Store`, `DataAddress`. Terminators: `Jump`,
  `Branch`, `Return`, `Unreachable`.
- Calls name a signature by ID, so the generator never re-derives an ABI from
  Jet types.
- `InlineAsm(dst?, args, regs, result, code)` is effectful, including when
  it has no result. The strict x86-64 assembler recovers audited asm bodies
  from their checked UTF-8 source spans. Inputs bind simultaneously to
  `rdi,rsi,rdx,rcx,r8,r9,r10`; selection snapshots all inputs with push/pop
  before emitting the template bytes. Allocation treats asm as a call.
  Supported templates contain word-register push/pop, mov (register,
  immediate, qword memory), movzx byte loads, syscall, indirect register
  calls, lock cmpxchg, xchg, and pause. Memory addresses admit a base register
  or named input with an optional signed-32-bit `+ displacement`.
  Callee-saved/frame registers, unsupported syntax, unbalanced stacks,
  misaligned calls, missing/duplicate return anchors, and named input reads
  after their bound register was written are registered native refusals:
  `LIRLoweringIssue` with the `inline asm:` prefix and the offending line.
  No bytes from a refused template enter the LIR. The OS-template witnesses
  in `Tests/Fixtures/InlineAsm.jet` use the actual `OS/Linux.jet` source.
  The one read-only frame-register form, `mov r64, rsp`, may expose storage
  owned by balanced template pushes; it is refused with an empty template stack.
  Byte stores use explicit low-byte register names (including sil/dil and
  r8b-r11b), with a REX prefix. Exact `Int` inputs/results must have matching
  high two bits; non-inline carriers stop through `jet_rt_panic_overflow`
  before any template memory access. Fixed-width I64/U64 leaves bypass this
  carrier check. Direct raw-word `#Import(c)` calls name the imported symbol
  in LIR; non-word carriers, callbacks, handles, and link-unit bridges remain
  refused rather than acquiring an implicit adapter.

`LIR/Lint.jet` checks a module before any generator reads it: table IDs,
valid layouts, signature agreement of entry parameters, calls, returns and
jump arguments, single definition, dominance of every use, no use after a
`Move` or `Drop`, no `Copy` of a value with drop glue, Bool conditions, and
the shape of drop glue. `LIR/Print.jet` prints the text form used in tests.

## MIR -> LIR lowering

`lir_lower_entry(program: MIRProgram, entry: MIRFunctionID) -> LIRLowering`
lowers the functions the entry reaches into one `LIRModule`, appends the
generated drop glue, and runs the LIR lint on the result; `LIRLowering.entry`
is the entry's LIR function. `LIRLowering.issues` names the function and the
construct for everything the lowering cannot express yet, and carries the lint
findings; the module may only be handed to a code generator when `issues` is
empty. The lowering never decides meaning again (D-TIER-ONEIR1): every
decision below reads facts MIR already carries (checked types, ownership,
Prelude routes, phis, drop operations).

### Entry-rooted monomorphization

`Lower/Mono.jet` and `Lower/Reach.jet` prepare the executable inventory in one
FIFO that discovers functions, concrete types and render edges together
(`lower_prepare_entry`). A generic function, and every method of a generic
type, is instantiated once per distinct concrete binding a reached direct call
supplies: the same instances rustc monomorphizes from the Rust emitter's
generic output. The binding is the callee owner's applied arguments (a method
of `Pair<Int>` binds Pair's parameters) followed by the call's own type
arguments. An instance's body is the original's with every type-bearing row
substituted and every local ID (block, value, place, op) kept; originals stay
borrowed from the program.

MIR function IDs are stable. An original keeps its ID; an instance's ID is
`stable_id("mir-function-instance", key)` over its original ID and its bound
types' structural fingerprints (nominal declaration IDs, ordered arguments,
leaf facts; no checked identities), so the lowering derives every direct call
target from the call site (`lower_instance_id`) and keeps no target table.
LIR rows are the reached originals in declaration order, then instances in
first-request order, then glue. Instance LIR functions are named
`<key><bound types>` (`identity<Int>`); no instance crosses the runtime ABI.
A substituted type whose shape changed, and an identity-less bound type that
replaces a checked binder, take the identity
`mir_type_id("mono:" + fingerprint)`, so ID-only type operands resolve to the
instance, except a tuple, which keeps its checked
identity because its MIR field IDs derive from it. A generic declaration's
concrete definition (`lower_type_def`) substitutes its fields, payloads and
alias or distinct base and keeps its declaration and field IDs. Glue is keyed
by role and fingerprint, so two declarations of one display name never share
glue. Render edges select the Printable/Display implementation whose receiver
matches the rendered type exactly or is the generic template of its
declaration, and reach that method's instance for the receiver.

The structural renderer carries an explicit Display/Show/Debug mode through
members. Debug does not select a user Display/Printable implementation: it
quotes text, renders optional values as `Val(...)` / `None`, and uses
`Ok(...)` / `Err(...)` for told reports. Tuples, records, variants, lists and
maps recurse in that mode; require diagnostics use the same Debug renderer.
Runtime-owned handles need their generated JetDebug adapter, not a structural
inspection of the host allocation. `Fixtures/Display.jet` covers Debug text,
optional and told reports, and its independence from user Display/Show methods.

Generic ancestry (each new function or type specialization along a request
chain) is limited to 128, rustc's default recursion limit: polymorphic
recursion such as `fn expand<T>() { expand<List<T>>() }` is a compile issue
naming the leaf request and its chain. A missing target, an owner or function
type-argument arity mismatch or an unbound type parameter is an issue too, and
a failed preparation lowers nothing.

### What Cranelift does today

`crates/jet-jit/src/jit/functions_compile.rs` lowers MIR straight to Cranelift
IR with these choices, which the Jet lowering keeps or replaces as noted:

- Every MIR value is one Cranelift value, almost always `i64`; Bool is `i8`,
  Char `i32`, Float `f64`. Places are Cranelift stack slots; phis are block
  parameters passed on each edge (`edge_args`). The Jet lowering keeps block
  parameters for phis and replaces stack slots with SSA values (below).
- `Int` is the exact integer: a word holding an inline integer in
  [-2^62, 2^62) or a tagged big-integer handle. Add, Sub and Mul take a native
  fast path when both operands are inline and the result is inline, and call
  the runtime route otherwise; comparisons are native when both are inline and
  use the runtime's three-way order otherwise. Kept as is.
- Strings, lists and maps are handles into the JIT's resident heap; a string
  literal is allocated in that heap at compile time and embedded as a constant
  handle. Replaced: a literal is static data plus one runtime call.
- Structs, tuples and payload enums are runtime heap records built with host
  calls (`struct_new`, `set_field`, then `trait_object_tag`); Options and
  Results are result-arena handles, or a packed `payload + 1` word in some
  places. Replaced: compiled code owns plain heap boxes and reads fields with
  loads (below), so the runtime keeps no record descriptors for user types.
- Prelude routes are looked up by module and member in the JIT's own host
  table (`call_prelude_values`); panics call the `trap_panic` host; drops call
  typed-owner host glue (`drop_typed_owner_value`). Replaced: calls name the
  compiled runtime's symbol, and drop glue for user boxes is generated code.

### Value layouts

| Checked MIR type | LIR carrier | Layout | Drop when owned |
|---|---|---|---|
| `Int`, ranges/tags/quantities over Int | exact-Int word | `i64` (Integer 8) | none (big integers are runtime-managed) |
| `Char`, `IntN(_, 64)` | word | `i64` | none |
| `Bool` | flag | `i8` (Integer 1, 0 or 1) | none |
| `Unit`, `Never` | no LIR value | — | — |
| `String` | runtime handle to a C-layout `{ptr, len, cap}` UTF-8 header (byte counts, words at offsets 0/8/16 on x64) | `ptr` | `jet_rt_string_drop` |
| `RangeCursor`, `IterCursor` | runtime handle | `ptr` | `jet_loop_cursor_drop` |
| struct, tuple | inline aggregate: pointer to storage holding the stored fields at natural offsets (`Lower/Values.jet`) | `ptr` | generated glue on the storage (frees nothing) |
| enum, every variant without payload | discriminant word | `i64` | none |
| enum with a payload variant | heap box: discriminant word at 0, payload members at natural offsets from 8 | `ptr` | generated glue |
| `T?` | null for absent, else a box holding the value's bytes at 0 | `ptr` | generated glue (null-safe) |
| `Result<T, E>` | box: 1 (Ok) or 0 (Err) at 0, payload at 8 | `ptr` | generated glue |
| `[T]` | box: `[len, cap, data]`, elements `max(8, size(T))` bytes apart | `ptr` | generated glue |
| `[K:V]` | the `[T]` header over `(key, value)` entry tuples in ascending key order (BTreeMap's); lookups are a binary search (`Lower/Maps.jet`); keys are exact Int, String or word-sized scalars | `ptr` | `[T]` glue over the entries |
| `View<T>`, `ViewMut<T>` | the `[T]` header `[len, 0, data]` in its holder's storage, `data` at the window's first element of the owner's buffer (`Lower/Lists.jet` lower_view_element); capacity 0 marks a buffer it does not own; List reads read it unchanged; a materializing `Copy` into `[T]` clones the elements | `ptr` | none |
| `View<str>` | an owned String handle holding the window's text: the `_view` kernels run their owned twins (`jet_string_after`, `jet_string_before`, `jet_unicode_trim`); costs one allocation the zero-copy `&str` avoids, otherwise unobservable (a window is only read) | `ptr` | `jet_rt_string_drop` |
| `Distinct`/`Alias` types | as their base | — | — |

Result boxes reserve at least one eight-byte payload word after the tag,
including zero-sized payloads. Runtime marshaling uses the same minimum,
alignment and size as native construction and drop glue.

`Compiler/JetRuntime/Source/Text.jet` owns the native text exports. Headers are
24-byte allocations aligned to 8; buffers are allocated with alignment 1 and
freed with their recorded capacity. Empty buffers have pointer 1 and capacity
0. Builders transfer their header unchanged on finish and preserve self-append
when growing. Static bytes use Rust-compatible lossy UTF-8 conversion; range
slices count Unicode scalars, not bytes. Split iterators clone their borrowed
input once and produce owned pieces lazily; nonempty `rsplit` materializes
rightmost nonoverlapping matches, then yields the pieces left-to-right.

Sizes: a word, handle or box pointer is 8 bytes (align 8), a Bool 1 byte, Unit
0. An inline aggregate is 8-aligned and padded to whole words, so copies move
words; a member sits at the next multiple of its alignment, and a recursive
(declared boxed) edge is one pointer to a wrapper with the present Option
layout. Box payloads and list elements use the same byte layout, so an inline
aggregate stored in one is its bytes. Boxes come from `jet_rt_alloc(size,
align)` and return to `jet_rt_free(ptr, size, align)` with the same size (one
size function per box kind serves construction and glue). A variant's
discriminant is its index. Unions and trait objects are not lowered yet and
are reported; generic instances lower as their concrete instances
(above). Function values are closure boxes ("Closures" below).

Inline aggregates. A `Struct`/`Tuple` builds into a fresh `StackSlot`. The
storage's holder owns it: the value never frees it, and MIR `Drop`/`Copy` of an
inline value call its drop glue (`drop.<key>`, `(ptr)`, drops the owned members,
null-safe) or clone glue (`clone.<key>`, `(source, target)`, copies the words
then replaces owned members by copies into storage the caller supplies). Root
writes, reads and moves pass the pointer, as for boxes; only a loop can make a
`StackSlot` overwrite a live value, so every inline slot and phi gets frame
storage of its own in the entry block, and each back edge (to a block of lower
or equal reverse-postorder rank) copies the carried inline values into that
storage (through temporaries when several are copied, so swaps are safe; a
slot's null placeholder stays null; a conditional back edge gets a block of
its own for the copies). Moving an inline member out of a projection copies
it to storage of its own and zeroes it in the parent so glue skips its owners.
Inline parameters pass the pointer; an inline result is written through a
hidden first parameter pointing at the caller's storage (the callee returns
nothing). A Write argument of inline type passes its storage, which the
callee updates in place.

A value's drop action comes from its MIR ownership fact: owned values
(`drop != NoDrop`, not a read or write borrow) carry the type's drop; borrows,
copies and every value of a type without drop carry none. MIR `Drop(value)`
becomes LIR `Drop` when the value carries a drop action and nothing otherwise.

Drop glue is one LIR function per box type, `drop.<canonical type key>`,
taking the box pointer and returning nothing (`LIRDrop.Function`). It drops
every owned member of the active shape (testing the discriminant for enums and
Results, and skipping null for Options), then frees the box. Glue for nested
boxes and inline members names further glue; the list grows while it is
generated.

### Locals, places and SSA

Every root place (a local, a parameter or a temporary, with no projections)
becomes a *slot* carried as an SSA value. `WritePlace` and `ReplacePlace` set
the slot (`ReplacePlace` first drops the old owned value); `ReadPlace` and
`MovePlace` name the slot's current value without an instruction. Every block
except the function entry receives one block parameter per carried slot, after
its phi parameters, and every jump passes the current value of each slot.
A slot that is unassigned, moved out, or whose value was dropped holds a zero
placeholder with no drop action; MIR's checked definite assignment and its
drop-live flags guarantee the placeholder is never used as a live value, so
the lowering needs no liveness or assignment analysis. A field place
(`place.f.g`) loads the intermediate boxes and then loads or stores the last
field. MIR value ids are hashed, so the lowering keeps MIR -> LIR value maps.

### Control flow

MIR blocks are lowered in reverse postorder from the entry. A block containing
`Never` or `Todo` ends with that operation's non-returning runtime call and
`Unreachable`; blocks reachable only through it are not lowered. When the MIR
entry block is itself a jump target, a separate LIR entry block takes the
function parameters and jumps to it (LIR forbids jumps to the entry).

| MIR terminator | LIR |
|---|---|
| `Jump`, `Continue`, `Break` | `Jump` with phi and slot arguments |
| `Branch(c, t, e)` | `Branch` on the Bool `c` |
| `Switch(subject, arms, otherwise)` | a chain of `Branch`es, one per arm condition, in order, then `Jump otherwise` |
| `Return(v)` | `Return(v)`, or `Return` with no value when the function returns Unit |
| `Unreachable` | `Unreachable` |
| `Yield` | not lowered yet |

### Operations

| MIR operation | LIR sequence |
|---|---|
| `Parameter(i)` | the entry block's parameter for MIR parameter i (Unit parameters have none) |
| `Constant` Int/Bool/Char | `Const`; exact-Int literals outside the inline range use a runtime constructor fixup, including frozen statics |
| `Constant` String | `DataAddress` of the literal's bytes, `Const` length, call `jet_rt_string_from_static(ptr, len) -> String` |
| `Copy` | the same value for word, flag and tag carriers; `jet_rt_string_clone` for a String; clone glue for boxes; a fresh `StackSlot` filled by clone glue for an owned inline aggregate (a borrowing copy names the source) |
| `Move`, `AttachTag`, transparent `Convert` | the same value |
| exact-Int `+ - *` (Prelude route, arity 2) | inline test (`(x ^ (x << 1)) >= 0` on both operands), native op (Mul also checks `Overflows`), inline test of the result, else the route's runtime symbol; joined in a block parameter |
| exact-Int other ops (division family, pow, shifts, bit ops) | call of the route's runtime symbol with `(left, right)`, plus `(file_ptr, file_len, line)` for routes of arity 4 |
| exact-Int comparison (Primitive) | native `Compare` when both are inline, else `jet_int_compare(l, r)` compared with 0 |
| word `+ - *` (Primitive) | `Binary`; with `overflow: Trap`, an `Overflows` test branches to `jet_rt_panic_overflow` first |
| word `/ %` (Primitive) | zero check to `jet_rt_panic_division_by_zero`, then a minimum-divided-by-minus-one check to `jet_rt_panic_overflow` (x86-64 `idiv` faults on it), then `Div`/`Rem` |
| word bit ops and shifts | `Binary And/Or/Xor/Shl/Shr` |
| Bool, tag and word comparisons | `Compare` |
| String `==`/`!=` | `jet_rt_string_eq(l, r) -> Bool` |
| `And`/`Or` on Bools | a branch on the left operand joining the right operand or the decided constant |
| `Unary Neg` | exact Int: `0 - x` with the exact fast path; word: `Sub` |
| `Unary Not` | Bool: `Compare Eq x, 0`; word: `Xor x, -1` |
| `Struct`, `Tuple` | `StackSlot` of the aggregate's size, each field moved in at its offset (bytes copied for inline members) |
| `Field`, field `ReadPlace` | `Load` at the field's offset; an inline member is its address (`PointerAdd`) |
| `Enum` | tag enum: `Const` discriminant; payload enum: box, `Store` discriminant, payload members moved in at their offsets |
| `EnumIs` | `Compare Eq` of the discriminant (loaded from offset 0 for boxes) |
| `EnumPayload(index)` | read of the member at its offset (an inline member's address) |
| `Present(v)` / `Absent` | box holding v's bytes / null pointer `Const 0` |
| `OptionIsSome` / `OptionValue` | `Compare Ne` against null / read at offset 0 |
| `ResultOk` / `ResultErr` | box, word at 0 = 1 / 0, payload at 8 |
| `ResultIsOk` / `ResultValue` | `Load` offset 0 and `Compare Ne 0` / read at offset 8 |
| `BuildString(parts)` | `jet_rt_string_builder_new`, `..._push_static(b, ptr, len)` per literal, `..._push(b, text)` per Display or Debug interpolation, `..._finish(b) -> String` |
| `Semantic(Print(call, v))` | Display text of v (the String itself, or `jet_rt_int_to_string` / `jet_rt_i64_to_string` / `jet_rt_char_to_string` / `jet_rt_bool_to_string`, dropped after the call), then the route's symbol (`jet_term_write_stdout_line`) with `(text, flush = 1)` |
| `Call` of a user function, method or associated function | `Call Function(id)` with the callee's signature; Unit arguments are omitted |
| `Call` of a Prelude route | `Call Runtime(symbol)` |
| `LoopRangeInit/HasNext/Value/Advance` | the route's symbols; init takes `(start, end, step, has_step, exclusive)` |
| `Todo` / `Never` | the Todo route's symbol with the location / `jet_rt_unreachable(ptr, len)`, then `Unreachable` |
| `ScopeEnter`/`ScopeExit` | nothing for Unsafe, Impure, AssumeDeterministic, Layout and DebugOnly scopes; others are reported |
| `Drop(v)` | LIR `Drop` when v carries a drop action |
| `InitializeUninit` | nothing |
| `Closure(f, captures)` | a closure box (below) with the code and release helper addresses (`FunctionAddress`) |
| `IndirectCall(v, args)` | `Load` of the box's code word, then `Call` through it (`LIRCallee.Value`) with the box first |
| `BuildMap(entries)` / Map `Index` / `m[k] = v` | an empty header, then per-type helpers `map.set` / `map.find` (a missing key stops) / `map.set` with a copy of the borrowed key; `get`, `has_key`, `insert`, `pop`, `len`, `is_empty` call `map.get`, `map.find`, `map.insert`, `map.remove` or read the length |
| `Capture(slot)` | the capture's box field (by value) or the address it holds (by reference) |

Every other operation (the remaining Map methods, trait objects, pattern
captures, the remaining semantic operations) is reported as not lowered yet,
by name.

### Closures

A function value is a pointer to a heap box `[reference count, code
address, release helper, captures...]` (Lower/Closures.jet). The code takes
the box as a hidden first parameter. A closure body (a function with capture
parameters) is the code; any other function used as a value runs through a
generated thunk that ignores the box. Clone glue counts a reference; drop
glue runs the release helper on the last one, which drops the by-value
captures and frees the box. A capture parameter that borrows (ReadBorrow,
WriteBorrow or Write access) is stored as its place's address. Captured
places are memory cells: in the body the box field or referenced storage, in
the creating function frame storage every read and write goes through, so
both see one value. On x86-64 `FunctionAddress` is `lea [rip + disp32]` and an
indirect call is `call r10`.

Closure-taking methods (MIR `ClosureMethod`, Lower/Combinators.jet) are
loops over the borrowed receiver that call the box with each element
borrowed, as the Prelude kernels call `f(&item)`: List `filter` (kept
elements copied), `map`, `count_where`, `fold` with an accumulator that owns
nothing, and Option `map`. A closure moved into the method is dropped after
the loop. Other closure methods are reported.

`Lower/Parallel.jet` lowers `try_map` as an owned, short-circuiting walk:
the first error is moved to the result, the produced prefix is dropped,
and unused inputs and a moved callback are released. `para_map` allocates
one output buffer and generates a typed range callback; the raw
`jet_rt_native_para_walk` bridge lends its stack environment until the
existing parallel chunk scheduler joins every worker. Inputs and captures
are borrowed, each worker writes disjoint output slots, and result order
is the source order. Callbacks with a checked stop across native frames
remain refused until the task stop-record protocol supports that edge.

### Calls into the compiled runtime

The lowering calls the compiled runtime (Prelude and Core, D-EXEC1) by symbol
with the System V convention: every argument and result is one word, a Bool
is returned in the low byte, Strings are handle pointers, and a source
location is three words `(file_ptr, file_len, line)`. A Prelude route's symbol
is the last `::` segment of its MIR symbol (`jet_std::jet_int_add` ->
`jet_int_add`); the compiled runtime exports a C-ABI entry under that name.
Symbols the lowering itself names:

| Symbol | Signature |
|---|---|
| `jet_rt_alloc` | `(size, align) -> ptr` |
| `jet_rt_free` | `(ptr, size, align)` |
| `jet_rt_string_from_static` | `(ptr, len) -> String` |
| `jet_rt_string_clone`, `jet_rt_string_drop` | `(String) -> String`, `(String)` |
| `jet_rt_string_eq` | `(String, String) -> Bool` |
| `jet_rt_string_builder_new`, `_push_static`, `_push`, `_finish` | `() -> ptr`, `(ptr, ptr, len)`, `(ptr, String)`, `(ptr) -> String` |
| `jet_rt_int_to_string`, `jet_rt_i64_to_string`, `jet_rt_char_to_string`, `jet_rt_bool_to_string` | `(word) -> String`, Bool takes the flag |
| `jet_rt_float_to_string`, `jet_rt_float_debug` | `(Float in xmm0) -> String`, Jet Display / Debug of a Float |
| `fmod`, `pow` (C library) | `(Float, Float) -> Float`, Float `%` and `**` |
| `floor` (C library) | `(Float) -> Float`, rounds the quotient for Float `/%` |
| `jet_int_compare` | `(Int, Int) -> word` (-1, 0 or 1) |
| `jet_loop_cursor_drop` | `(ptr)` |
| `jet_rt_panic_overflow`, `jet_rt_panic_division_by_zero` | `()`, does not return |
| `jet_rt_unreachable` | `(ptr, len)`, does not return |
| `jet_rt_main` | `(entry: fn()) -> exit code`: runtime environment setup, then `entry` inside the AOT entry boundary (atexit hooks, parked tasks, stop reports) |

The exports live in `crates/jet-codegen/src/Prelude/Core/CAbi.rs` (module
`jet_c_abi`), compiled into the same `jet_runtime` crate AOT links, so every
export adapts carriers and calls the Prelude function generated Rust calls.
Ownership: an Int word is the exact-Int carrier (range-loop routes convert to
and from the native kernel); a String handle is borrowed by every parameter
except `jet_rt_string_drop` and `jet_rt_string_builder_finish`, which consume
it (`jet_term_write_stdout_line` borrows, so the caller drops the text it
printed); `jet_loop_cursor_drop` frees a range cursor. A runtime stop renders
its report and exits the process; nothing unwinds into generated frames.
Prelude and Core routes go through adapters that `Tools/agent/gen-c-abi-routes.mjs`
generates (`CAbiRoutes.rs`, with the carrier table `Lower/RuntimeRoutes.jet`).
When a route is ported to `Compiler/JetRuntime`, its direct `#Export(c)`
function is the adapter: it takes raw fixed-width carriers and uses the
canonical route C symbol. `gen-runtime-exports.mjs` records that definition in
the single `exports.list`; `gen-c-abi-routes.mjs` keeps the shared lowering
metadata but omits its Rust adapter body. Both tools read the runtime-only
`sources.list`. The route generator checks the Jet declaration against the
derived C ABI, including every flattened list argument and result out-slot,
and fails rather than silently skipping a Jet-owned route whose ABI cannot
be derived. Exact `Int`, typed `String`, write borrows, failures, and mismatched
parameter/result widths are not raw route signatures. `--check` validates
ownership and signatures as well as generated-file freshness.
`node Tools/agent/gen-runtime.mjs <allparts.rs> --check` checks trap literals,
the export list, and routes in one keep-going invocation. Without `--check`
it regenerates them in dependency order. Narrow worker and held-job checks
can use `Tools/agent/lane-check.sh --runtime-generators <allparts.rs>` through
the cheap-check lane; this mode does not invoke Cargo.
Text has one C-ABI carrier, `JetCText`, not Rust's unspecified `String` layout.
Owning headers and buffers are allocated/freed by `jet_rt_alloc`/`jet_rt_free`;
empty buffers have capacity zero and a non-null dangling pointer. The Rust
boundary copies a returned Rust String into this domain, and copies text out
before consuming a carrier as a Rust-owned String. Read-only `&str` kernels
borrow its UTF-8 bytes; remaining `&String` signatures receive a Rust-owned
temporary copy. Builders grow with the runtime allocator and finish by
transferring the unchanged header. Runtime-handle String field projections
write a borrowed header into caller stack storage; that header and its bytes
are never freed or grown, and owning reads clone it. Native compiler text
marshaling continues to use `jet_rt_string_data`/`jet_rt_string_byte_len`,
never Rust layout. This contract applies to the linked runtime pack; the
standalone freestanding image runtime is separate and retires with L3.
An owned result from a Rust `&'static str` kernel copies directly into the
same E4 header and byte buffer via `text_handle`, without an intermediate
Rust `String`, including when that text is an Option/Result payload.
List result slot buffers use the same `jet_rt_alloc`/`jet_rt_free` domain,
including lists returned inside an Option or Result. Rust `Vec` storage is
only temporary marshaling storage: adapters copy its words into the shared
runtime allocation before returning a buffer to native lowering. Empty lists
keep the null-buffer/zero-length convention.

A Jet type that generated Rust holds as a native runtime type crosses as an owned
`Box<T>` handle. The generator reads Context.rs and predicates.rs name tables and the
explicit names in MIRRust `rust_apply_type`, using the same canonical-Core probe as
MIRRust. Each such type gets `jet_rt_handle_{drop,clone,display,show,debug,eq}_<T>`
adapters where the runtime type supports the operation. A `&mut T` kernel parameter
takes the caller's handle as `HandleMut`, and the kernel mutates the value inside that
box. A tuple result is written item by item through one out pointer per item.
Explicit MIR Write arguments must match `HandleMut`; ordinary read and move arguments
must not. Canonical Core identities override a source layout only when the generated-Rust
name rules replace that exact identity; source-owned enums and records retain their
native constructors and matches. Tuple adapters write zeroed word slots, and lowering
copies each item into the tuple's naturally aligned native field offset.
A MIR-defined Core record (the general `Err`, `IOError`, `IOContext`; schemas in
`Lower/Records.jet`) keeps its native layout and crosses a route as its runtime
type's handle: a generated `record.out` helper builds it through
`jet_rt_record_new_<R>[_<Variant>]` (String handles and String/Int Option boxes
borrowed, nested record handles taken), and `record.in` rebuilds the native value
through `jet_rt_record_tag_<R>` and `jet_rt_record_get_<R>_<member>` (owned
results; an Option getter returns its tag and writes one out slot). A fieldless
Core enum member crosses as its variant index. A record argument's handle is
dropped after the call; a returned handle is dropped after conversion. Records
render through their runtime type's own Display, Show and Debug, as generated
Rust does, and a failing entry's `Err` reaches `jet_rt_entry_error_exit_err` the
same way.
Recursive record collection members use generated `jet_rt_record_seq_*`
holders while crossing the boundary. Byte lists, record lists and `(key:
String, value: Record)` entry lists retain their native List layouts; the
holders expose only raw length, indexed read and capacity-reserved append.
`DataTree.Object` remains an insertion-ordered entry list, including duplicate
keys. `DataEntriesToMap` copies borrowed entries into the ordinary native Map
in source order, so the last duplicate wins without changing the original
DataTree. Record getters lend collection and nested-element storage while
rebuilding native values; constructors consume the freshly built holders.


Lazy collection routes use the existing `Collections.rs` and `LoopCursor.rs`
kernels through `CAbiIter.rs`; lowering does not recreate their iteration or
failure policy. A copied descriptor tree carries stride, clone/drop and checked
trait callbacks plus recursive element/error descriptors. Slots up to three words
are stored inline; a borrowed cursor projection retains the buffered owner until
advance, while the native cursor's taken flag prevents returning it twice.
Callbacks follow MIR parameter access: Read lends a slot, Move transfers an owned
input or clones a borrowed input, and the returned consume mask disarms only moved
owners. Write uses a genuinely mutable owned local; a borrowed kernel input may
not be made mutable by casting its shared reference. That unsupported adaptation
is reported as a native lowering failure, not silently changed to Read.
Scalar Show callbacks reuse the ordinary value renderer rather than the
aggregate-only display helper. `is_sorted_by` snapshots a borrowed List into
the same iterator kernel as a lazy source, evaluates keys only as that kernel
requires, and uses the key descriptor's native ordering callback.
Map callbacks borrow separate key/value operands in stored key order; Move
parameters receive copies. Search exits at the first deciding predicate,
map/filter append ordered entry tuples, and fold uses the same accumulator
replacement/ownership convention as List fold. These inline loops remain
refused in stop-point modules until they can return without reading a stopped
callback's unwritten result.
`Tests/prepare-call-runtime.mjs --allparts` refreshes the current C ABI, generated
routes and iterator/task adapters together for full-pack proofs; its default mode
retains an emitted seed's paired route set. Generated Rust programs carry only
`CAbi.rs`: the routes name optional runtime modules a program gets only when it
uses them, so `CAbiIter.rs`, `CAbiTask.rs` and `CAbiRoutes.rs` exist only in the
all-parts native runtime crate.

Task and channel adapters share the iterator's typed slot descriptors rather
than introducing another erased value ABI. A native SendFn body writes its
result directly into an uninitialized slot; Sema's sendability facts authorize moving
the body and its captures to a scheduler thread. The existing task, task-group
and channel kernels retain responsibility for ordering, cancellation and error
policy. Consuming operands register as ownership sinks before lowering place
reads: a task group cannot destructively consume a borrowed list header, and
join/detach cannot consume a borrowed task. Loaded code and its destructors must
remain mapped until every task body and returned value owner has finished.
Task pause/resume delegate to the same scheduler control as Rust tasks and
do not consume the handle. A by-value List cursor can move each task handle
out once; its transfer flag excludes that slot from later cursor cleanup,
while skipped and remaining handles retain their ordinary join-on-drop duty.
Heterogeneous `task.all` carries each child's own result descriptor and moves
successful values into the checked tuple's field offsets. Readiness tables
normalize to owned receiver/timer Lists; blocking and else-arm waits delegate
to the canonical selector and initialize their arm/payload tuple only after
the runtime returns normally.
Opaque runtime-owned enums do not expose a native discriminant or payload
layout. Their construction, variant tests and payload projections report a
native unsupported issue unless an explicit record adapter supplies that layout.
`Closed` is such an enum (a zero-sized `Box` may have dangling address 1), so it
stays an opaque handle and is never read at pointer + 0; the rejection fixtures
cover Closed variant tests and construction and assert that no zero-offset load
is emitted. `TaskFailure` has a record schema (Records.jet, over the canonical
`jet_task_failure_abi`): task joins and groups rebuild it in its native layout,
so its payload projection lowers natively, as the projection fixture checks.

Linking: the runtime rlib plus a staticlib wrapper crate
(`extern crate jet_runtime;`, `--crate-type staticlib`) gives
`libjet_runtime_c.a`; link the object with it and `-lpthread -ldl -lm`.

### LIR additions for the lowering

The lowering added to LIR: `Div Rem And Or Xor Shl Shr` binary operators;
`Overflows(dst, op, l, r)` (Bool: signed Add/Sub/Mul overflow);
`Load(dst, base, offset)` and `Store(base, offset, value)` through a Pointer;
`DataAddress(dst, data)` with `LIRModule.data` (read-only bytes);
`LIRTerminator.Unreachable`; `LIRDrop.Function(id)` for generated glue;
`StackSlot(dst, size, align)` (the address of frame storage of its own, live
until the function returns; Write-argument cells use it); and
`lir_layout_pointer()`. The lint checks each of them, accepts a null pointer
`Const 0` and pointer `Eq`/`Ne` comparisons, and checks that drop glue takes
one pointer and returns nothing.

## x86-64 instruction selection

`X64/Select.jet` compiles one linted function after register allocation
(`x64_compile_function`; `x64_compile_module` lints first and stops on any
finding). Every value is a 64-bit register or frame slot; Bools are
zero-extended words, so a branch tests the whole register.

| LIR | x86-64 |
|---|---|
| `Const` | `mov r, imm32` (sign-extended) or `movabs r, imm64` |
| `Add Sub Mul And Or Xor` | `mov dst, l` then `add/sub/imul/and/or/xor dst, r` |
| `Div Rem` | `mov rax, l; mov r11, r; push rdx; cqo; idiv r11; pop rdx` (Rem takes rdx) |
| `Shl Shr` | `mov rax, l; mov r11, r; push rcx; mov rcx, r11; shl/sar rax, cl; pop rcx` |
| `Compare` | `cmp l, r; setcc r8; movzx r, r8`, or fused into the block's `cmp` + `jcc` when the branch is its only use |
| `Overflows` | `mov rax, l; add/sub/imul rax, r; seto r8; movzx` |
| `Load` / `Store` | `mov r, [base + off]` / `mov [base + off], r`; one-byte layouts use `movzx` / byte `mov` |
| `DataAddress` | `lea r, [rip + disp32]`, a data relocation |
| `Call` | stack arguments pushed last to first (8 bytes of padding first for an odd count), Float arguments `movq xmm_i, r`, then parallel moves into rdi, rsi, rdx, rcx, r8, r9; `call rel32` with a function or runtime relocation; `add rsp` for the stack arguments; a Bool result is `movzx rax, al` first (System V defines only al), a Float result `movq rax, xmm0` |
| Float `Add Sub Mul Div` | `movq xmm0, l; movq xmm1, r; addsd/subsd/mulsd/divsd xmm0, xmm1; movq dst, xmm0` (Float `Xor` is the bit operation, used for negation) |
| Float `Compare` | `ucomisd` with the operands ordered so `a`/`ae` answer `< <= > >=` (false on NaN); Eq is `sete` and `setnp`, Ne `setne` or `setp`; never fused into the branch |
| `Convert` | `cvtsi2sd` (word to Float) or `cvttsd2si` (Float to word, truncating) through xmm0 |
| `Drop` | a call to the value's runtime or module drop glue |
| `Move` / `Copy` | a register or slot move |
| `Jump` / `Branch` | parallel moves for the block parameters; `jcc`/`jmp rel32`; a taken edge that needs moves gets a stub after the fall-through path |
| `Return` / `Unreachable` | the epilogue / `ud2` |

The lowering already checks division (zero, and -1 against the minimum), so
`idiv` never faults. rax, r10 and r11 are the selector's scratch registers and
never hold allocated values; rdx and rcx stay allocatable because the
instructions that need them save and restore them around their use. Float
values live in general registers as their IEEE bits and pass through
xmm0/xmm1 only around SSE2 instructions, so the allocator has one register
class. Parameters follow System V: general registers, then xmm0-xmm7 for
Floats, then `[rbp + 16 + 8k]` for stack arguments. Memory-class values and
Float32 are reported as selection gaps (`x64_selection_gaps`), not
miscompiled. `X64/Encoder.jet` holds the encodings.

## Register allocation

`X64/RegAlloc.jet` is linear scan (Poletto and Sarkar) over one conservative
interval per value. Blocks are laid out in function order; block-level
liveness (iterated to a fixed point) extends each interval across every block
the value is live through, so loops need no special case. Intervals are
scanned by start; an interval expires strictly before the next starts, so a
result never shares a register with its operands. Values live across a call
(or a `Drop`) may only take callee-saved registers (rbx, r12-r15); others
prefer the caller-saved pool (rcx, rdx, rsi, rdi, r8, r9). When no register
is free, the interval ending last is spilled to its own frame slot for its
whole life. Cranelift runs regalloc2 with live-range splitting; O0 accepts
whole-interval spills for compile speed, and O1 is where splitting belongs.

## Frames

`push rbp; mov rbp, rsp`, then the callee-saved registers the allocator used,
then one `sub rsp` for spill slots and StackSlot storage, padded so rsp is
16-byte aligned at every call. Spill slot k lives at
`[rbp - 8 * (saved + k + 1)]`. StackSlot storage lies below the spill slots,
each at the next multiple of its alignment (rbp is 16-byte aligned after the
push), and a StackSlot is `lea reg, [rbp - disp]`. Every return
restores rsp with `lea rsp, [rbp - 8 * saved]`, pops the saved registers and
rbp, and returns. Entry parameters arrive in the System V argument registers
and are moved (in parallel) to their allocated homes. The frame keeps rbp as
a frame pointer, so stack walks and debuggers work without unwind tables.

## Relocations and images

Compiled functions carry rel32 relocations to a module function, a runtime
symbol, or module data (`X64RelocTarget`). `Image/Link.jet`
(`x64_link_text`) lays functions out 16-byte aligned (int3 padding), places
runtime functions and data after them, and patches every rel32. The result is
position-independent, so the same bytes serve every image kind:

- **Static executable** (`Image/ELF.jet`, `x64_static_executable`): a
  read+execute `PT_LOAD` segment at 0x400000 holding the headers and code,
  and, when the module has data, a read+write `PT_LOAD` for it (the linker
  starts the data on a page of its own, so no page is writable and
  executable; startup fixups store String handles into it); no section
  headers. `_start` calls the entry function and passes its word result (or
  0) to `exit_group`. Runtime calls link against the freestanding image
  runtime below.
- **Relocatable object** (`Image/Object.jet`, `x64_relocatable_object`): an
  ELF64 `ET_REL` with `.text`, `.rodata`, `.rela.text`, `.symtab`, `.strtab`
  and an empty `.note.GNU-stack`. A global `main` calls
  `jet_rt_main(entry)`; every runtime call is an undefined global with an
  `R_X86_64_PLT32` relocation and every data address an `R_X86_64_PC32`
  relocation against `.rodata`. The in-process linker below binds it to the
  one compiled runtime; `cc program.o libjet_runtime_c.a -lpthread -ldl -lm`
  remains the external reference.
- **In-memory image** (`Image/Memory.jet`, `x64_memory_image`, for `jet run`
  and tiered execution): the linked block of the module's functions, one
  import stub per runtime symbol the code calls, and the data. A stub is
  `jmp qword [rip + 2]; int3; int3` followed by its 8-byte address slot
  (16 bytes, so every slot is 8-byte aligned); runtime calls are rel32 calls
  to the stub. The image records the entry offset, the page-separated data
  boundary, each import's symbol and slot offset, and data pointer relocations.
  Loading (`Image/Loader.jet`) resolves all imports and validates boundaries
  before mapping, copies into anonymous read+write memory, rebases pointers,
  binds import slots, then changes only code pages to read+execute. Data stays
  read+write and never executable, including frozen runtime-handle fixup cells.
  Origins honor over-page data alignment. A program runs through
  `jet_rt_main(base + entry)`; no page is writable and executable together.
  `Tests/load-memory-image.c` performs the same steps in C, out of process,
  as the reference the Jet loader is checked against.

`x64_runtime_current` resolves the host's exported resident runtime;
`x64_runtime_open` resolves a loaded shared runtime. The host retains that
runtime until no mapped frame, value or destructor can use it. An unresolved
symbol is a load failure before any generated code runs, not a request to
retry on another execution engine.

`Image/Calls.jet` invokes a checked callable through a selector-generated
System V adapter, rather than reimplementing the register/stack ABI in the
host. It takes a pointer to 8-byte argument cells and one zeroed result cell;
the selector places Integer, Pointer and Float arguments and captures the
return carrier. `lir_lower_callable` preserves failure carriers instead of
normalizing them as program entries do. Compiler-value marshaling borrows
runtime String bytes through `jet_rt_string_data` and
`jet_rt_string_byte_len`, copies before dropping the native owner, and never
depends on Rust's private String representation.
Signed and unsigned 64-bit compiler values preserve their full bit patterns;
unsigned values above `i64::MAX` detach as decimal `BigInt` compiler values.
Runtime effects, stop recovery, execution limits and composite ownership are compiler-call
prerequisites, not properties inferred from a successful memory mapping.

The canonical runtime exposes a comptime request scope:
`jet_rt_comptime_begin(fuel, max_depth)`, `step(owner, start, end)`,
`work(units)`, `enter(owner, start, end)`, `leave()`, `end()` and
`stop_write()`. `owner` is a dense row in the retained original MIR program, so
the compiler maps a callee's span through its canonical function/module
association rather than interpreting it against the prep caller. Fuel is
shared across native callback threads; depth and the current source site
belong to each native stack. Pure runtime kernels charge `work`, not only
generated MIR steps. Ordinary runtime execution never activates the scope;
`begin` enters the request's own task control on the calling thread.

An exhausted budget is a recoverable stop (unified host/stop contract, #4720;
"Stop points" below): the runtime records the request's first budget stop,
publishes it as BudgetFuel/BudgetDepth on the current task control, spends the
remaining fuel so every thread's next charge stops too, and returns; the MIR
Propagate after the budget operation takes the stop edge and canonical cleanup
runs. A runtime kernel iterating for the request leaves through the canonical
cancellation transfer, which its export's guard turns back into the rail. A
panic is fatal: the child flushes output, writes its stop record and exits
(81 runtime panic, 101 foreign/host panic). `stop_write()` writes a budget
stop's record. Records travel on private descriptor 3 as the body's decimal
byte length, a newline, then the body: one JSON object with `kind`
(Cancelled, Deadline, BudgetFuel, BudgetDepth, Panic, HostFault, Trap),
`jct_status` (1 runtime panic, 2 fuel, 3 depth, 4 foreign/host panic),
`owner`, `start`, `end` (null when absent) and the exact UTF-8 `message`. A
parent controller must concurrently drain stdout/stderr and enforce a wall
envelope for kernels not yet instrumented. `Tests/comptime-runtime.c`
exercises the actual exports in exec'd children, including successful scope
reuse and fuel charged from a callback thread.

## In-process linking

`Image/StaticLink.jet` (`x64_link_executable(names, bufs)`, #4549) links
relocatable objects with the prebuilt runtime archive set into a static
x86-64 executable without a system linker, compiler or child process.
`Image/Archive.jet` reads GNU `ar` archives in place (member table, `/` and
`/SYM64/` symbol indexes, `//` long names); inputs are told apart by their
magic bytes.

The runtime archive set is what the compiled runtime needs at the final
link: the runtime rlib (the `jet_runtime` crate with its C-ABI exports, one
object per codegen unit), the Rust standard library rlibs of the same rustc,
musl's `libc.a` and `libgcc_eh.a`/`libgcc.a`. The rlibs target
`x86_64-unknown-linux-gnu`; on x86-64 musl's static libc serves them once
the linker supplies the few glibc-only names below. What the linker does:

- **Resolution.** All given objects are loaded, then archive members are
  pulled through the indexes for every strong undefined reference until none
  can be satisfied (all archives form one group). Weak definitions yield to
  strong ones, common symbols are allocated in `.bss`, the first copy of a
  COMDAT group wins and the members of later copies are discarded. Objects
  with more than 65279 sections (the runtime's codegen units have ~63k) use
  extended section numbering (`e_shnum == 0`, `SHN_XINDEX` and
  `.symtab_shndx`).
- **Synthesis.** When the archives provide musl's `__libc_start_main` and no
  object defines `_start`, the linker adds its startup object: `_start`
  passes `__jet_link_main` to `__libc_start_main`; `__jet_link_main`
  registers `.eh_frame` with libgcc (`__register_frame_info`), runs every
  `.init_array` entry with glibc's `(argc, argv, envp)` arguments (the
  gnu-target std captures `argv` there; musl's own init loop sees an empty
  array) and tail-calls the program's `main`, which calls `jet_rt_main`.
  The startup object also defines `__dso_handle`. For references still
  undefined after resolution the linker writes the allocator shim rustc
  emits at its final link (`__rust_alloc`/`dealloc`/`realloc`/`alloc_zeroed`
  jump to the default allocator's `__rdl_*` in the same mangled `__rustc`
  crate; `__rust_no_alloc_shim_is_unstable_v2` returns), aliases glibc's
  LFS64 names (`fstat64` -> `fstat`, identical on x86-64) and answers
  `_dl_find_object` with "not found" so libgcc's unwinder uses the
  registered tables. The gnu-target std's resolver names two glibc functions
  musl lacks: `__res_init` returns 0 (musl rereads resolv.conf on every
  lookup) and `gnu_get_libc_version` returns "2.26", the first glibc that
  does the same, so std never needs the reload. Synthetic code is written as
  an ELF relocatable object and loaded through the same path as every input.
- **Collection.** Sections live from `_start`, the init/fini arrays,
  `SHF_GNU_RETAIN` sections and the CIEs' personality references, following
  relocations. `.eh_frame` is split into CIE and FDE records; an FDE is kept
  exactly when the function it describes is live (and its LSDA then is), and
  every kept FDE's CIE pointer is recomputed. The same pass over each live
  section's relocations assigns GOT slots and reports undefined symbols.
- **Layout.** ET_EXEC at 0x400000 with file offset = address - base, three
  page-aligned `PT_LOAD` segments: read-only (headers, `.rodata*`,
  `.gcc_except_table*`, the kept unwind records plus a zero terminator),
  read+execute (`.text*`, padded with `int3`), read+write (`.tdata`, the
  init/fini arrays, `.data.rel.ro*`, the GOT, `.data*`, then `.bss*` and
  common symbols), plus `PT_TLS` (`.tdata` + `.tbss`) and `PT_GNU_STACK`. No
  section headers.
- **Relocation.** `R_X86_64_64`, `PC32`, `PLT32`, `32`, `32S`, `PC64`,
  `GOTPCREL`, `GOTPCRELX`, `REX_GOTPCRELX`, `GOTPC32`, `GOTOFF64`, `TLSGD`,
  `TLSLD`, `DTPOFF32/64`, `GOTTPOFF`, `TPOFF32/64` and `SIZE32/64`; anything
  else is a link error. Static-executable relaxations, chosen as GNU ld
  chooses them for a static non-PIE executable: TLS general- and
  local-dynamic sequences become local-exec (`mov rax, fs:0`), initial-exec
  loads become immediates; a REX.W `mov` through the GOT becomes `lea` (any
  GOT type) or, for `REX_GOTPCRELX`, `mov reg, imm32`; `call`/`jmp` through
  the GOT become `addr32 call`/`jmp; nop` only for `GOTPCRELX`, so rustc's
  plain `GOTPCREL` calls keep their slot. Thread-pointer offsets follow
  variant II: an address minus the end of the aligned TLS block. On the
  hello/stop/unwind probes the code the linker writes disassembles to the
  same instruction sequence as GNU ld's output from the same inputs (only
  addresses and padding differ).

### Runtime packs

Linking against the archive set reads ~100 MB and ~1M section headers per
link. `Image/RuntimePack.jet` (`x64_link_runtime_pack(names, bufs,
extra_roots)`) does everything that does not depend on the program once per
toolchain and writes the result as one self-contained ELF relocatable
object, the runtime pack, shipped beside the runtime archive; a program link
is `x64_link_executable([program.o, pack])` and reads nothing else.
`Tools/packaging/prebuild-runtime.sh` writes it as
`jet-runtime/<key>/jet_runtime.pack.o` beside `jet`, in the directory of the
release-profile runtime rlib it was linked from (the runtime cache's prebuilt
layout), and records that key in `jet-runtime/native.key`. The key covers the
runtime's Rust text and `rustc -vV`, neither of which the native build has, so
the Jet-hosted `jet build` reads the key from that file in one place
(`jet_cli_runtime_key`) and refuses the build with E2105 when it is missing.

- Resolution is finished: members are pulled for the roots (the C ABI the
  runtime archive defines, `_start`, `__jet_link_main`, `__tls_get_addr`,
  and the caller's extra names such as `memcpy`), COMDATs, weak and common
  symbols, the allocator shim, the LFS64 aliases and the libc shims are
  settled, and the startup object is included. Only the roots stay global;
  every other definition becomes a local symbol, so the pack's symbol table
  is its export table plus its imports (`main`, the linker-defined names,
  weak references).
- Only the sections reachable from the roots are kept, each still its own
  section with its relocations rewritten against the pack's symbols, so the
  program link still collects what the program does not reach. Section
  names are reduced to what classification and init-array priority read.
- One `.eh_frame` holds the FDEs of kept sections and the CIEs they use;
  `.note.GNU-stack` marks the stack non-executable.
- Every real section index at or above `SHN_LORESERVE` (`0xff00`) is encoded
  as `SHN_XINDEX` with its full index in `.symtab_shndx`, including indexes
  numerically equal to `SHN_ABS`/`SHN_COMMON` and indexes above 65535.
  Absolute/common symbols use distinct internal sentinels; those reserved
  values are interpreted only in the original 16-bit `st_shndx` field, never
  after resolving an extended index. `Tests/run-link.mjs <outdir> --indices`
  assembles the boundary round-trip tests.

The program link recognizes the startup object by the pack's
`__jet_link_main` export. Because the pack is plain ELF, GNU ld links it too
(`ld -static -e _start program.o pack.o` plus `--defsym` for the
`__jet_*`/`__EH_FRAME_BEGIN__` names), which is how the relaxations above
were compared. For the current runtime the pack is 3.0 MB (8998 section
headers, 66 exports); hello-world keeps 3136 of its sections.

Link time, measured with the linker compiled by `jet build --release` (the
Rust-emitting tier; not yet measured as Jet-backend output), inputs
already in memory: hello-world 67 ms, the runtime-stop probe 89 ms, against
13.5 s for hello-world linked from the archive set in the `jet run` tier
(~0.5 s from the pack there). Building the pack takes 0.55 s in the release
tier. Collection and relocation dominate, about equally; both are linear in
the live sections' relocations.

`Tests/run-link.mjs <outdir>` assembles `Tests/LinkRuntime.jet` with the
linker sources; `jet run unit.jet <output> <inputs>...` links and
`jet run unit.jet -- --pack <output> <archives>...` builds a pack, each
reporting read, link and write times.

## Freestanding image runtime

`Image/Runtime.jet` hand-assembles, over raw Linux system calls, every symbol
in the table above for images that link neither libc nor the compiled
runtime: mmap/munmap memory, `[len][bytes]` Strings, a doubling string
builder, decimal, Bool and UTF-8 Char display, `jet_term_write_stdout_line`
with unbuffered `write`, range cursors with the Prelude's `has_next`,
`value` and `advance` semantics, inline-Int `jet_int_add/sub/mul/div/compare`,
and runtime stops that print `error[CODE]: message` to stderr and exit with
70 like the compiled runtime. It has the same borrow and consume rules as the
C-ABI exports. Ints are inline only: a result outside the inline range stops
with E3010 naming the compiled runtime, which is where big integers live.

## Jet OS layer

Owner ruling 2026-10-01 (#4015): in-process execution and the future
Jet-written runtime build on one audited low-level OS layer written in Jet:
per-OS system call wrappers plus a call-code-address primitive allowed only
in audited unsafe code, internal to the toolchain (users get safe Core APIs on
top). `OS/Linux.jet` is that layer for Linux x86-64, the first target.

### API

A system call wrapper returns the kernel's result: a value, or `-errno` in
-4095..-1 (`os_failed(result)` tells them apart).

| Entry | Meaning |
|---|---|
| `os_syscall(number, a1, ..., a6) -> Int` | One raw system call. `#[Unsafe, FFI(asm)]`; every input is pushed before an argument register is written, so operands may live in any register. |
| `os_read(fd, address, count)`, `os_write(fd, address, count)` | read(2), write(2) on memory the caller owns |
| `os_map(size, protection)` | Fresh zeroed `MAP_PRIVATE \| MAP_ANONYMOUS` pages, `size` rounded up to pages; `protection` is a sum of `OS_PROT_READ/WRITE/EXEC` |
| `os_protect(address, size, protection)`, `os_unmap(address, size)` | mprotect(2), munmap(2) over whole pages |
| `os_exit(status)` | exit_group(2) |
| `os_store_word(address, value)` | One 8-byte store. `#[Unsafe, FFI(asm)]` |
| `os_load_word(address)`, `os_load_byte(address) -> Int` | Read one word/byte from caller-owned readable memory. `#[Unsafe, FFI(asm)]` |
| `os_copy_in(address, bytes)` | Copy a `[U8]` in, one little-endian word at a time |
| `os_call_address(address, argument) -> Int` | Call-code-address (below). `#[Unsafe, FFI(asm)]` |
| `os_call_address_two(address, first, second) -> Int` | Call-code-address with two System V word arguments. `#[Unsafe, FFI(asm)]` |
| `os_library_open(path)`, `os_library_symbol(library, name)` | Bind runtime exports by name through the dynamic linker (today's runtime only, below) |
| `os_page_round(size)`, `OS_PAGE_SIZE`, `OS_PROT_*`, `OS_SYS_*` | Helpers and constants |

FFI assembly functions use the ordinary braced function body immediately after
their return type, as the `os_call_address` example below shows. Source gates use
the dev release Jet checker so this OS layer and the rest of the assembled
compiler share the same accepted grammar.

Threads come next, on the same `os_syscall`: `clone3` with a stack from
`os_map` and a second assembly entry that switches to the new stack and
calls the start address, and `futex` wait/wake for the runtime's locks and
parking. They land with the first Jet runtime code that needs them.

### Who may use it

No new surface: visibility does it. The layer's items are private to the
`jet_backend` package, its only Jet consumer while the runtime is Rust; files
of one package share one namespace (D-MOD-CYCLE1), so `Image/Loader.jet` uses
them and nothing outside the package can. When the Jet runtime needs the
layer, it moves to a sibling package `jet_os` whose items are
`pub(package)`: D-PUBPKG1 exports such items to the sibling packages of the
same payload (the compiler packages and the runtime) and never to downstream
user packages, which is exactly the owner's import rule. Two bootstrap-only
gaps keep it package-private until the Jet-hosted checker takes over (Rust
freeze, no fix there): the Rust parser rejects a marker before
`pub(package)` (`#[Unsafe("…"), FFI(asm)] pub(package) fn` reports E0355,
the marker "cannot attach at the Type site"; the Jet parser's
`parser_parse_item_modifiers` accepts markers and qualifiers in any order),
and the Rust name ledger scopes `pub(package)` by source directory
(`package_scope_for`), narrower than D-PUBPKG1's sibling packages.

On top of visibility, every machine-level entry is an `#Unsafe("reason")`
function, so each call sits in an audited `#Unsafe` block that says why the
address and arguments are valid (I1); inline assembly also needs
`use core.mem` in its file (E3102) and the `FFI` authority to run (E1803).

### Call-code-address

`os_call_address(address: Int, argument: Int) -> Int` calls the System V
function at `address` with one word argument and returns its word result. It
is spelled with the existing inline-assembly mechanism (D-FFI-ASM1,
D-FFI-ASMOPS1) rather than a new builtin, so it adds no syntax and no ballot:

    #[Unsafe("calls machine code; …"), FFI(asm)]
    fn os_call_address(address: Int, argument: Int) -> Int {
        """
        mov rdi, {argument}
        call {address}
        mov rax, rax ; -> return
        ; clobbers rcx, rdx, rsi, rdi, r8, r9, r10, r11
        """
    }

The result register rax and the listed clobbers cover every caller-saved
general register, so the callee may use them all; the stack is call-aligned
on entry to an assembly body.

- **Audit rule.** An `#FFI(asm)` function without `#Unsafe("reason")` is
  E3215, and calling an `#Unsafe` function outside an `#Unsafe` block is
  E3103, so every call site names why `address` is executable code of that
  signature (the loader: "`main` is the runtime's `jet_rt_main` and `entry` is
  the image's checked no-argument entry").
- **Sema.** Registration checks the assembly contract (Rust
  `Sema/Registration.rs`, Jet `Sema/InlineAssembly.jet`): integer-only
  signature (E3222), `use core.mem` (E3102), every parameter bound as a named
  operand, one `; -> return` destination, audited clobber registers (E3223).
- **Lowering and emitters today.** MIR carries the function as a foreign row
  (`MIRForeignLanguage.Assembly`); the Cranelift JIT and the rustc path call
  it through the hidden cdylib trampoline (`jet_inline_<name>`, generated by
  `emit_asm_wrapper` in `crates/jet-pkg-model/src/FFI.rs` as `asm!` with
  `in(reg)` operands and `out` clobbers). That runs real `syscall` and `call`
  instructions inside `jet run` with no Rust change.
- **The Jet backend.** An `#FFI(asm)` function lowers to its body assembled
  in place by `X64/Encoder.jet`, with the checked operands bound to the
  registers the allocator gives the parameters. The layer deliberately uses
  only `mov`, `push`, `pop`, `syscall` and `call` with register and `[reg]`
  operands, which the encoder already emits. LIR has no inline-assembly
  instruction yet; until it does, the backend reports `#FFI(asm)` functions as
  a lowering gap, like every other unlowered construct.

The ruling names a "compiler intrinsic"; the assembly function is that
primitive with the same unsafe-only rule and no new surface. A dedicated
builtin (typed signatures instead of one word in and out) would be new
surface and would need a ballot first.

### Backing today and later

- **Today (Rust runtime).** The system calls are the assembly bodies above,
  run through the trampoline. The compiled runtime's C-ABI exports
  (`crates/jet-codegen/src/Prelude/Core/CAbi.rs`) are not in the `jet`
  binary, so the loader opens the runtime as a shared library
  (`libjet_runtime_c.a` linked whole into `libjet_runtime_c.so`) with
  `os_library_open` and binds each import with `os_library_symbol`: dlopen
  and dlsym, the layer's only C (two `#FFI(c)` functions, D-FFI-INLINE1).
  The Rust side needs nothing beyond the existing C-ABI exports.
- **Later (Jet runtime, compiler built by the Jet backend).** The assembly
  bodies become inline `syscall` and `call` instructions in the compiler's
  own image. The runtime is Jet code compiled into the same image, so its
  export addresses are link-time facts; `os_library_open` and
  `os_library_symbol` are deleted, and the layer is system calls only.

## In-process loader

`Image/Loader.jet` is ordinary Jet on the OS layer:

- `x64_runtime_open(path) -> X64Runtime` opens the runtime library.
- `x64_load_image(image, runtime) -> X64LoadedImage` checks the image (entry
  inside it, every slot an aligned word inside it, every import resolvable;
  an unknown symbol is a reported error, not a crash), maps
  `os_page_round(text.len())` bytes read+write, copies the text, stores each
  import's address into its slot, and flips the mapping to read+execute.
- `x64_run_image(loaded, runtime) -> Int` calls
  `jet_rt_main(base + entry)` through `os_call_address` and returns the
  sign-extended `int32_t` status; a runtime stop ends the process from inside
  the runtime, as in the C loader.
- `x64_rebind_slot(loaded, slot, address) -> Bool` re-points one slot: the
  slot's page goes read+write (not executable), the word is stored, and the
  page returns to read+execute.
- `x64_unload_image(loaded) -> Bool` unmaps the block.

`strace -f -e trace=mmap,mprotect,munmap` of `jet run load/load.jet hello`
shows the image page mapped `PROT_READ|PROT_WRITE`, then
`mprotect(..., PROT_READ|PROT_EXEC)`, then the program's `write`; a rebind
shows `PROT_READ|PROT_WRITE` then `PROT_READ|PROT_EXEC` on that page; no
mapping of the whole process is ever writable and executable at once.

## Stop points

The unified execution host (#4720) runs session turns and native comptime in
one persistent child. Interrupts, deadlines and comptime fuel/depth limits
stop recoverably inside that child and leave through explicit MIR cleanup;
a root panic or trap invalidates the child and the host restarts it. A
SendFn task's intentional program panic instead remains the Rust-tier
`TaskFailure.Panicked(reason)` value for its joiner. Task, `#Test` body and
property-generator ABI callbacks share `jet_native_callback_body`: it enters
the thread-local task-body scope, invokes native code, and resumes a delivered
stop only after that code returns. Callback results remain uninitialized
until this gate succeeds. Test runners use their canonical run/finish rails;
the root C-ABI guard does not turn traps into successful callbacks. MIR carries
the stop paths, so no backend call falls through after a flag is set:

- `Poll { kind: Check | Propagate, resume, stop }` (MIR.jet `MIRPollKind`,
  MIR.rs `MirPollKind`). `Check` delivers a pending session interrupt; it
  sits at function entry and at each loop continue target, where every live
  value is owned by a cleanup frame. `Propagate` directly follows a can-stop
  operation (`mir_operation_can_stop`: calls, Core calls, callbacks, waits)
  and reads only a stop that operation already delivered, so a result that
  initialized before a late interrupt stays owned until the next Check.
- `Stopped` ends the stop edge after canonical cleanup. The function returns
  with the stop still delivered on the task control and no result written;
  its value ABI does not change.

Legality (Legality.jet `mir_pass_validate_stop_points`, MIR.rs
`validate_stop_points`): a Propagate follows a can-stop operation; every
read, projection or drop of its result (a phi input by its incoming edge) is
dominated by the resume edge; a function with Stopped has one Shield depth
per block and zero at every Stopped. Cleanup, Shield and transaction regions
carry no stop points (cleanup is Shield-masked).

Native host and runtime compilations produce stop points
(`JetCodegenProgram.stop_points`; Rust checked TIR's
`lower_checked_native_mir_program_for`). The ordinary Rust/web emitters,
evaluator and inliner refuse them; their programs retain their existing
unwind-based semantics. The golden harness's `JET_NATIVE_STOP_POINTS=1`
requests a fresh native MIR dump (`JET_DUMP_NATIVE_MIR`) without changing
the Rust reference artifact. O0 lowers `Check` to `jet_rt_session_poll()`,
`Propagate` to `jet_rt_session_propagate()` (any nonzero word takes the stop
edge) and `Stopped` to a return. Shield exit and checked program stops are
can-stop operations too, so their delivered transfer is observed before
the next ordinary instruction.

Task callbacks use OCB1's zero-input consumed mask (zero). Their result cell
starts non-owned and uninitialized; only a zero delivered-only Propagate
allows its bytes to become an owned `NativeValue`. A stopped body resumes
the canonical cancellation, deadline or program-panic transfer only after
returning to its Rust task boundary. Body release is Shield-masked. A typed
native task-panic marker distinguishes a program's exact reason from an
unknown runtime/host payload; it uses rail word 5 only inside a native
SendFn body. Session/comptime root panics and host faults remain fatal.
Clone and trait callbacks observe the same delivered-only status before
reading their result. Bool slots initialize one byte, not their stride's
padding: native marshaling reads only that byte (and reads no bytes for Unit).
A conditional checked stop seals its runtime arm back to the Propagate join.
Diagnostic-only owned text is released before that poll; borrowed source
messages retain their original owner.

Comptime budgets are can-stop operations too (`comptime_budget`, compiler
callables only): a Prelude call `jet_rt_comptime_enter(start, end)` after the
parameters and `jet_rt_comptime_step(start, end)` at each loop continue
target, each followed by its Propagate, and `jet_rt_comptime_leave()` (depth
bookkeeping, no poll) before each Return. MIR carries the site's source span;
O0 prepends the function's owner row in the program it lowers, the dense row
the stop record names. When fuel or depth runs out the runtime publishes a
BudgetFuel/BudgetDepth stop on the task control and returns; the Propagate
takes the stop edge.

### Persistent native sessions

A session must preserve runtime values, not replay earlier initializers or
deserialize their printed forms. The host retains one native-pack runtime and
all installed code until reset. `crates/jet-codegen/src/NativeSession.rs`
provides the Linux x86-64 host; its turn entry uses the existing
pointer-to-cells adapter for `(session: u64) -> status: u64`. A generated turn
manifest records the resident runtime artifact's SHA-256. Loading another
runtime instance, or an image with a different runtime identity, is refused.
This internal loader accepts only trusted compiler output, not user plugins.

The canonical slot implementation is
`crates/jet-codegen/src/Prelude/Core/CAbiSession.rs`, embedded by both runtime
prefix producers. Its exported `jet_rt_session_*` operations own aligned
canonical-layout storage, keyed by binding ID and MIR type-key digest plus
size/alignment. `reserve` returns uninitialized storage; checked construction
precedes `publish`. `resolve` borrows an initialized place for one turn; `take`
transfers its payload's ordinary ownership and clears its initialization flag.
`end` releases unpublished/moved storage without drop glue. `retire` and
`destroy` drop initialized payloads exactly once through their original glue.
Binding IDs increase with declaration order and are never reused. Reset retires
slots in reverse order before unloading code; in-turn retire/drop ordering is
explicit in MIR. Slots are worker-thread-confined; the ABI does not manufacture
thread safety or change language borrow rules.

This ABI is not an implicit transaction: explicit writes to live mutable
bindings retain their ordinary meaning. A recoverable interrupt must propagate
through MIR-visible polling and normal cleanup, leaving the host alive. The
current comptime failure boundary exits the process; it cannot implement that
contract. No `longjmp` may skip owners' drops. Panic/trap terminates the worker
and invalidates its session. The host/slot substrate alone does not activate a
REPL cutover or claim interruptible native lowering.
## Tiered recompilation

The tier model (D-TIER-FORM1, D-TIER-ONEIR1) recompiles a hot function at a
higher level from the same MIR. Every in-memory image calls module functions
through a per-function entry slot, the role `hotswap(true)` gives Cranelift's
GOT today:

- `x64_memory_image` places a slot table after the data, starting on its own
  page: one 8-byte entry per module function, in function order. Every call
  between module functions becomes `call qword [rip + disp32]` (FF 15) to the
  callee's entry instead of a rel32 call; the image records each entry's
  offset beside the import slots.
- The loader fills each entry with `base + function offset` while the block
  is writable, then maps the code read+execute and the table read-only, so
  the table never shares a page with code.
- A swap compiles the new body into a fresh image whose calls go through the
  same table entries (they are its imports), loads it, and rebinds the
  function's entry: table page read+write, one store, back to read-only.
  Frames still running the old body finish there; new calls take the new
  body; the old mapping is released once no frame can return into it (at the
  next safepoint with no such frame).

Today `x64_rebind_slot` does the store-and-flip on any slot, and it is
exercised on an import slot (rebind hello's `jet_term_write_stdout_line`
slot, run the entry again, same output). Because import slots still live in
the code pages, it flips that page back to read+execute; it moves to the
read-only table pages, and a thread running code on a page being rebound can
no longer fault, once `x64_memory_image` emits the table above.
Static executables and objects keep direct rel32 calls.

## Rust source emitter (release builds)

Release builds (D-EXEC1) still go through rustc: `Compiler/JetCodegen/Source/Emit/`
turns checked MIR into Rust source. Every local and SSA value is an
`Option<T>` slot declared at the top of the function, so a view value is an
`Option<&str>` or `Option<&[T]>` slot borrowing its base local's slot.

**The defect (#3941).** The emitter inherited MIRRust's body shape: one
dispatch loop, `'jet_mir_dispatch: loop { match pc { ... } }`, one arm per MIR
block. rustc's borrow checker reasons about the Rust control flow it is
given, and in that loop any arm may follow any arm. When a view is read in
one block and its base local is rebound or dropped in another (a loop
variable at the end of the iteration, a scope-end drop), rustc assumes the
read can follow the rebind and rejects the body with E0502 or E0506, although
MIR never takes that path and the Jet view checker accepted it. Valid Jet such
as

    loop line in xs {
        t :: line.trim()
        if t != "" && t.len() < 9 -> &out.push(~t)
    }

could not be release-built (`Core/app/app.jet` `auth_oauth` and
`footprint_atoms`, `Jetpack/Environment`). A view read in the block that
defines it never failed; the short-circuit `&&` puts the reads in later
blocks.

**Options weighed.**

- (a) Structured emission: emit straight-line Rust (labeled blocks, loops,
  `if`) for reducible control flow so NLL sees MIR's real paths; keep the
  dispatch loop only for irreducible flow.
- (b) View slots as `(base, start, len)` triples, re-sliced at each use: no
  borrow outlives a block, but every read pays a bounds check and slice
  rebuild, views into temporaries need an owned base slot, and every view
  operation in the emitter changes.
- (c) A lifetime-erasing helper (`&'static` via raw pointers) justified by the
  Jet view checker: one `JET_VETTED_UNSAFE` region with an audit note, but it
  makes generated code unsafe and turns any checker or lowering bug into a
  use-after-free instead of a compile error.

**Choice: (a).** It needs no unsafety, costs nothing per use, and changes only
control-flow emission. rustc and LLVM also get natural loops instead of one
`match` over a program counter, which they optimize better. Checked Jet bodies
come from structured source (`if`, `loop`, `match`, `break`/`next`, `?`,
`return`), so their CFGs are reducible; the dispatch loop stays as the fallback
for any CFG that is not, with the defect.

**Algorithm** (`ControlFlow.jet`, `jet_rust_emit_cfg_plan` and
`jet_rust_emit_cfg_run`; Ramsey, "Beyond Relooper", JFP 2022). Number the
reachable blocks in reverse postorder, compute immediate dominators (Cooper,
Harvey and Kennedy), and check that every retreating edge targets a block that
dominates its source; otherwise fall back. A block with a back edge into it is
a loop header; a block with two or more forward in-edges is a merge block.
Each block is then emitted once, walking the dominator tree:

- a loop header opens `'jet_loop_<id>: loop { ... }`;
- the merge blocks a block immediately dominates follow it, each after a
  labeled block `'jet_block_<id>: { ... }` that the earlier code leaves with
  `break 'jet_block_<id>`, outermost for the merge latest in reverse postorder;
- any other block is emitted in place at its only forward in-edge;
- a back edge is `continue 'jet_loop_<id>`.

Every block body still ends in an explicit transfer, so no code falls through
a label, and the last arm of a branch or switch follows the `if` instead of
nesting in an `else`: early-return chains and matches with hundreds of arms
(each arm test is its own MIR block) stay flat. A branch whose then-subtree is
printed in place while its else edge is a `break` or `continue` is negated, so
the subtree stays at the outer depth and only the short edge nests. The walk
runs on an explicit work stack, so body depth never deepens the emitter's call
stack; a body that would nest deeper than 256 levels (rustc's recursive passes
overflow their stack at about 1400 nested blocks) keeps the dispatch loop.
Phis keep reading `__jet_mir_dispatch_prev`; a predecessor sets it to its block
id only on an edge into a block whose phi has two different incoming values
(a phi whose rows all carry one value is that value). Unreachable blocks are
not emitted.

**Block-local slots.** A value slot whose type has no observable drop (Bool,
Int, sized ints, floats, Char, String) is declared at its `slot = Some(..);`
definition instead of the function top when every later mention stays in that
statement's lexical scope; a slot the body never mentions loses its
declaration. Drop timing of other types (guards, handles) is observable, so
they keep top-level slots.

**MIRRust parity and size.** MIRRust (`emit_structured_body`, `set_pc`,
`localize_value_slots`) applies the same plan, so the Rust compiler's stage-zero
output of the whole Jet compiler shrinks too. Measured by replaying the rules
on run 6's emitted `reference.rs`: 16.53M lines / 1135 MB in dispatch form,
10.20M lines / 893 MB structured (every one of the 14556 dispatch bodies is
reducible; deepest body 216 levels), 8.69M lines / 832 MB with block-local
slots. Derived `Decode` bodies (3.28M lines, about 78% never reached from the
compiler entry) are the next target.

**Native derived bodies.** A compiler-generated `Equatable` or `Comparable`
impl on a closed, non-generic struct or enum without computed fields is
printed from the type definition instead of its lowered template body
(`emit_native_derived_equal` / `emit_native_derived_compare`, mirrored by
`jet_rust_emit_native_derived_equal` / `_compare`). `Comparable` follows
Prelude/Derives.jet: fields in declaration order, `.Less` on `<` and
`.Greater` on `>` (an unordered Float pair falls through as equal), a field
whose type has a selected `Comparable` impl orders through that impl, and an
enum orders by variant ordinal before same-variant payloads. Any field type
outside that set keeps the template body. The Jet emitter also skips the free
function behind a natively printed method.

Derived `Encode` and `Decode` impls on such types (no Core or native host
projection; RenameAll is the only container attribute) are printed the same
way (`emit_native_derived_encode` / `_decode`, mirrored by
`jet_rust_emit_native_derived_encode` / `_decode`), through the Prelude helpers
`jet_decode_object_field`, `jet_decode_enum_candidate` and
`jet_decode_enum_payload` in `EncodingTraits.rs`. The bodies keep the template
semantics of jet-sema `Registration/Serde.rs`: a struct decode reports
`expected an object` for a non-object, otherwise decodes every field as
`FieldError.under(key, (tree.field(key) ?? Null).decode<T>())` and returns all
failures in field order; an externally tagged enum tries its variants in
order; every `Err` claims the template's journey origin. The printer reads the
field keys, literal strings and journey origin back from the lowered template
and keeps the template whenever they disagree, or when the body calls a user
function (a `validate` block or a default expression). Skipped, defaulted and
computed fields, `#[Flatten]`, `#[DenyUnknownFields]`, `#[Tag]` and
`#[Untagged]` keep the template.

**Codec reachability.** Derived serde impls whose type no emitted body
demands are skipped: a body demands an impl by calling one of its methods or
by naming its type anywhere in a call's type arguments (generic Prelude
codecs reach impls through trait bounds), and a demanded impl's methods are
scanned in turn (`Compiler/JetFoundation/Source/MIR/Reachability.jet`
`mir_demanded_codec_impls`, used by the Jet emitter for every program;
MIRRust `demanded_codec_impls`, enabled only for stage zero through
`MirRustExecutionConfig.prune_unreachable_codecs`). A missed demand fails
rustc with E0277 rather than changing behavior.

**Proof.** The repro above and both `Core/app/app.jet` patterns, compiled
from the Rust compiler's checked MIR through the Jet emitter (MIR from
`JET_DUMP_MIR`, converted by `Compiler/JetBackend/Tests/mir-debug.mjs`), give
bodies that rustc accepts; the same bodies in dispatch form fail with E0502.

## Testing

`Compiler/JetBackend/Tests/LowerFixtures.jet` builds small checked-MIR programs
by hand (hello, exact-Int arithmetic, locals with a loop and a phi, a switch,
a range loop, string interpolation, structs, payload and tag enums, Options),
lowers them, prints the LIR with `lir_print_module`, reports every issue, and
compiles each clean module (entry `run`) to `<name>.elf` (static),
`<name>.o` (for the compiled runtime) and `<name>.image` with its load
manifest `<name>.mem` (in-memory). Assemble, run and check with:

    node Compiler/JetBackend/Tests/run-lower-fixtures.mjs <outdir>
    cd <outdir> && jet run unit.jet
    [JET_RUNTIME_C_LIB=.../libjet_runtime_c.a [JET_RUN=jet]] node Compiler/JetBackend/Tests/check-native-fixtures.mjs <outdir>

Every fixture must end `<name>: lint-clean`, the run ends with `all fixtures
lowered lint-clean and compiled`, and the checker runs each executable and
compares its stdout and exit status with the fixture's Jet meaning. With
`JET_RUNTIME_C_LIB` it also links each object against the compiled runtime
and loads each in-memory image with `Tests/load-memory-image.c` (linked
against the same runtime, `-rdynamic` so the loader resolves imports by
name), and checks both the same way. With `JET_RUN` as well (the command
that runs jet), it links the runtime into `libjet_runtime_c.so` and loads
each image in process from Jet code: `run-lower-fixtures.mjs` also writes
the loader unit `load/load.jet` (the image type, `OS/Linux.jet`,
`Image/Loader.jet` and `Tests/LoadImages.jet`), and the checker runs
`jet run load/load.jet <name> libjet_runtime_c.so` in `<outdir>`.

Real programs: `Compiler/JetBackend/Tests/run-goldens.mjs <outdir>` takes every
Examples/features golden with an expected stdout, runs it with `jet run` and
`JET_DUMP_MIR=<path>` (a debug hook in
`crates/jet-codegen/src/Codegen/TIR/mir.rs` that writes the checked program's
Rust Debug form; inert when unset), converts the dump into the Jet MIR schema
with `Tests/mir-debug.mjs` (field and variant correspondence of the bootstrap
codec; the functions the entry reaches), lowers every case in one
unit (`Tests/GoldenLower.jet`, the backend files exactly as
`run-lower-fixtures.mjs` assembles them, plus a decoder generated from the
schema) and writes `<case>.o` and, when the freestanding image runtime serves
every import, the static executable `<case>.elf` that native `jet build`
writes. The run stage executes the static executable when there is one (no C
library, no rustc), otherwise links the object with `JET_RUNTIME_C_LIB`, and
compares stdout with the golden; each verdict names its leg (`static` or
`hosted`). `--stage` and `--until` bound the stages that run.
`<outdir>/summary.txt` counts pass, wrong, unsupported and harness verdicts and
ranks the blockers by how many goldens each one stops.
Each executed native case retains stdout, stderr and process status/signal/error
receipts so all wrong-output, stop and crash causes can be diagnosed as one batch.
Set `JET_MIR_EMIT=1` to regenerate checked MIR with the current reference
compiler's `emit --rust` seam without compiling or executing its Rust output.
The native executable still runs against the golden; the dump receipt is
explicitly an emission receipt, not a reference-execution pass.
For independently queued shards, `--case-list FILE` selects exact newline-separated
golden stems, rejecting empty, duplicate or unknown rows. Give each shard its own
output directory and decoder unit; share only the checked MIR input corpus.

An interrupted lowering batch can resume with `JET_LOWER_RESUME=1`: its original
`lower/unit.jet` is required and kept frozen; completed `.o` files and nonempty
issue reports are retained rather than lowered again. Replay runtime linking for
any object interrupted before its executable/link report was written, then run
the comparison over fresh working directories. The harness refuses new MIR
emissions, driver builds and shard processes while `~/.cache/jet-dev/proofq/PAUSE` exists; queue the
continuation through proofq after the compile gate, not as a direct heavy run.

Native-vs-Rust differential (#4619): `Tests/run-differential.mjs <outdir>`
reads a run-goldens output directory after its run stage and compares each
native run (`run/<case>.stdout`, exit status) with the Rust path's `jet run`
reference (`mir/<case>.ref`), not only with the golden. A native result that
differs from the Rust path is `native-diverges`, separate from a golden both
paths miss (`agree-off-golden`). The gate fails on any divergence; with
`--require-native` also on any golden the Rust path passes that native does
not agree on, and with `--baseline <differential.tsv>` on any case that
agreed before and no longer does. It writes `differential.tsv` and
`differential-summary.txt`.

Sanitizer mode (#4619): `Tests/run-sanitized.mjs <outdir> --runtime
<runtime.rs>` builds the prepared runtime text (the one the corpus's runtime
pack came from) as a staticlib with `--cfg jet_native_pack
-Zsanitizer=address -Zub-checks=yes`. `JET_RUNTIME_JET_OBJECT` must name the
same Jet object used in that pack; the strong-definition guard rejects
replaced Rust exports. Every native object links the Jet provider first, then
the sanitized remaining Rust archive, with `clang -fsanitize=address`.
Each golden retains every ASan/LSan report under `sanitize/reports/`. This
checks the remaining Rust runtime and libc, but rustc does not instrument
the Jet runtime object's internals. Native code itself is checked when
run-goldens ran with `JET_LOWER_SANITIZE=1`: GoldenLower then also writes
`lower/<case>.asan.o`, lowered through `LIR/Sanitize.jet`, which precedes
every Load and Store whose base is not a stack slot of its own function with
a call to ASan's outlined check (`__asan_loadN`/`__asan_storeN` on the
effective address and the access width); run-sanitized links that object
instead of `<case>.o` and records which one ran. Native frames carry no
redzones, so stack slots are not checked. A run without a report must also
reproduce the plain native run (ASan fills fresh heap blocks with `0xbe`, so
a native read of uninitialized heap diverges). The gate is zero reports and
zero divergences; `--no-leaks` drops LeakSanitizer and the summary records
it.

Fuzz corpus (#4619): `Tools/agent/safety-fuzz.mjs <outdir> --seed N --count K`
writes K type-directed generated programs and their expected stdout in the
Examples/features layout (`<outdir>/corpus/Examples/features/fuzz/`), then runs
run-goldens, run-differential and run-lint-corpus over it through
`JET_GOLDEN_FEATURES`. The expected output comes from a model evaluator with
i64 semantics; programs whose evaluation overflows, divides by zero, indexes
out of range or runs too long are redrawn, so every case is defined
behavior. `fuzz.tsv` adds the Rust path against the model (`model-agree`,
`model-differs`, `rust-error`) to each case's differential and lint
verdicts; the gate fails on any Rust-path error or model difference, any
native divergence and any lint finding. A case reproduces from its seed.

### Comparing the two front-end feeds

`JetDriver.jet_driver_compile_mir` runs the same checked pipeline and optimizer
without invoking the Rust emitter. The Jet CLI's native `build` selects this
path; release emission still selects `jet_driver_compile`. Host supplies the
exact versioned compiler identity through `JET_COMPILER_IDENTITY` (canonical
`name@version#build-id`), alongside `JET_TOOLCHAIN_ROOT` for Core sources.
Native builds hand the live `MIRProgram` to `lir_lower_entry`, generate the
object, and use the in-process linker with the packaged runtime pack.

The golden runner also accepts `--front jet`. It invokes the published
`candidate_jetc0` from `~/.cache/jet-dev/dev-artifacts.json`, or the explicit
`JET_FRONT_JETC0` candidate under proof. The bootstrap factory selects
`JET_FRONT_NATIVE_OUTPUT`, runs `jet_driver_compile_mir`, lowers the live
checked value and links it with `JET_RUNTIME_PACK` in that same process.
The `mir` stage therefore also performs native lowering/linking; the
`convert` and separate lowering-driver stages are skipped. The run stage
uses the same golden stdout/stderr/exit rules as `--front rust`.
A candidate without this native entry is reported as `no-mir`, not used as
a Rust-front fallback. Each case retains its canonical diagnostic receipt,
LIR, object, executable, frontend log and execution receipt.

```sh
JET_FRONT_JETC0=CANDIDATE/jetc0 JET_RUNTIME_PACK=RUNTIME/jet_runtime.pack.o \
  node Compiler/JetBackend/Tests/run-goldens.mjs OUT --front jet \
  --case-list FULL.cases
```

Build the candidate and runtime pack from the same composed tree. Matching
goldens do not prove full frontend equivalence; compare the two modes on the
same complete case list, retaining both `results.tsv` files and every
failure, before admitting parity.


`Tests/run-front-feed.mjs` selects **every** `pass` row in an explicit native
`results.tsv` baseline. It assembles the compiler source inventory,
`Tests/FrontFeed.jet`, and schema-generated observation codecs into one probe.
The Jet feed passes the driver's live MIR directly to the backend; only the
independent Rust feed uses Debug conversion. Both feeds link the same runtime
pack and run in separate, identically staged working directories with the
golden's stdin. The gate requires matching stdout, stderr, and status between
feeds, and successful stdout agreement with the golden for each feed.

```sh
node Compiler/JetBackend/Tests/run-front-feed.mjs OUT \
  --baseline NATIVE/results.tsv --rust-mir CACHED/mir --assemble
# Build OUT/probe/unit.jet with the reference compiler's release backend.
JET_RUNTIME_PACK=RUNTIME/jet_runtime.pack.o \
  node Compiler/JetBackend/Tests/run-front-feed.mjs OUT \
  --baseline NATIVE/results.tsv --rust-jet REFERENCE_JET --probe PROBE_BINARY
```

`--rust-jet` regenerates checked MIR for the staged source via `emit --rust`
and the MIR observation hook; it does not execute a different reference tier.
The Debug converter follows the bootstrap codec's memo bound split, comptime
field spelling, fixed-width trap-route derivation, and preformatted string
interpolation. A missing checked Copy fact or payload is a stale-schema error,
not an invented zero/default. Regenerate such cached dumps before differential
execution. `node Compiler/JetBackend/Tests/test-front-feed-codec.mjs` exercises
these mappings and strict token-stream observation without a full compiler
build.

Full MIR observations are retained per case. `gaps.json` and `gaps.tsv` rank
all observed schema, type, layout/ABI, ownership/drop, route, and generic
differences by the number of distinct affected goldens. IDs scoped to source
authority are compared through symbolic names where pairing is unambiguous;
ambiguous pairing is reported, never treated as equality. `--bootstrap JETC0`
provides a front-end diagnostic census before the probe can be built.
Preparation and a receipt-only census never establish differential parity;
only `summary.json` with `differential_verified: true` does.

### Native collection runtime

The runtime-only collection unit implements the current
`jet_rt_native_iter_*` slot ABI, not the Rust generic `jet_iter_*` names.
List and Map operations already emitted as typed LIR retain that lowering;
there is no unused parallel set of generic C exports.

Lists carry an inline `[len, cap, data]` header. Element descriptors have nine
words: stride, clone, drop, show, equality, comparison, kind, and two child
descriptors. Iterator owners copy the descriptors because the caller's
descriptor storage may expire. Pulls use reusable payload slots; Bool copies
one byte and Unit copies none. Moves, borrowed snapshots, closure release,
early termination, and buffered cursor stride have the same ownership rules
at the slot boundary. Collection buffers crossing that boundary belong to
`jet_rt_alloc`/`jet_rt_free`, including buffers from remaining Rust adapters.

Laziness remains operation-specific. Map/filter/take/skip and cursor pulls are
lazy; repeat/cycle/shuffle, fallible map/filter, keyed dedup/sorted checks, and
chunk-while retain the reference kernels' eager consumption. Integer averages
sum fixed-width values in a signed two-word accumulator before one binary64
rounding; exact-Int averages borrow the numeric layer.

The remaining progress-state and comptime-fuel leaves are opaque policy seams
owned by the host/session layers, not Rust iterator owners. Their Rust bodies
are seed/verification paths scheduled to die at cutover. The collection exit
harness checks the Jet object's definitions, unresolved pack imports, all
selected baseline stems, raw slot witnesses, and the count of newly passing
collection stems; preparation is not behavioral proof.

Lazy cursor initialization and advancement may invoke user callbacks. Both
are can-stop MIR operations: their results are usable only on the immediately
following Propagate poll's resume edge. Prefetched HasNext/Value operations
remain non-stopping. A cancelled map callback must report task cancellation,
never return a short successful collection or silently end a for-loop.

### Jet-authored runtime object

`Compiler/JetRuntime` is a separate package, assembled from its own
`sources.list`; it is never part of the compiler bootstrap inventory.
`JetBackend/Tests/run-runtime.mjs` sends that checked library directly through
the Jet frontend's library-object hook. The explicit `exports.list` is checked
against adjacent `#Export(c)` declarations and scalar C signatures. I64/U64,
U8, Bool and Float/F64 are admitted (F64 arguments/results use the System V
xmm ABI); the annotated exact-Int bounds-index seam is the sole exception.
The runtime's private allocator root uses the shared MIR `.Global` lowering.
`jet_rt_alloc_init` is a void/no-argument constructor in `.init_array.00000`.

`prebuild-runtime.sh` keeps the Rust-tier seed/verification rlib separate from
`libjet_runtime_native.rlib`, built from fresh all-parts source with
`--cfg jet_native_pack`. The latter imports the Jet allocator and is packed
after `jet_runtime_jet.o`; the strong-definition check rejects any replaced
Rust export. Rust-tier allocator bodies are seed/verification inputs only,
recorded for deletion at cutover, not a second native-pack implementation.

`JetRuntime/Tests/check-object.mjs` checks exact exports and the audited
`imports.list`, including the allocator constructor relocation.
`run-object-fixtures.mjs` prepares actual checked-source I64 global store/load
exports, an F64 ABI identity, and the allocator tests. A foreign caller tests
the globals; executable startup and an entryless `dlopen` library inspect the
allocator root before any explicit init call, then check repeated init leaves
it unchanged. The pack-linked `run-cabi-driver.mjs` preserves all C-ABI trap
cases, while the Rust-tier cargo driver only calls remaining Rust-tier routes.
`run-alloc-parity.mjs` separately checks invalid-layout exit 70 and allocation
failure `SIGABRT`, including byte-identical stderr against the Rust seed.
Process-edge stdout flushing uses a dedicated lock and error word in the
allocator's initialized root page, never a new allocation on the stop path.
