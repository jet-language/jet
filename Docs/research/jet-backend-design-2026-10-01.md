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

Merge-lost native session helpers have been restored to their canonical
providers: checked native value reads, slot writes and allocation receipts in
`OS/Linux.jet` (589ecd28b, with word-range validation from 4612eae5e), and
parent/child Core startup in `Image/Loader.jet` (82fe5a735). LIR debug facts,
builder source lines and x64 variable ranges are restored from e0628be87;
optimizer and sanitizer constructors retain those facts across rewrites.

Native `split_write`, `get_disjoint_write`, and `edit_disjoint` lower their
disjointness proof directly to LIR before producing any mutable descriptor.
The borrowed `ViewMut` representation remains `[len, 0, data]`: no receiver
elements are copied or owned by the returned windows. Indexed selection
checks bounds in request order, then detects duplicates in sorted order and
restores request order; its descriptor allocation doubles as proof scratch.
`edit_disjoint` checks cardinality before bounds and overlap, invokes its
callback exactly once only on success, and releases a moved callback on all
normal and checked-stop paths. `Tests/Fixtures/Disjoint.jet` supplies one
production MIR-to-native witness for each operation, including error
precedence, backing-store mutation, endpoints, and empty selection.
The `run-lower-fixtures.mjs --disjoint-lowering` mode lowers and lints those
same MIR programs without a pack; it does not stand in for executing their
native outputs with the admitted runtime pack.
Native Sema keeps fallback payload bindings distinct from their outer source
expression and binds a loop item's `ViewMut` arguments only on the successful
positive type match. This preserves the lending-view escape and arity checks
without relying on names outside their pattern scope.
Or-pattern alternatives compare binding names and types independent of payload
slot order; optional subjects retain their carrier while matching nested enum
alternatives. The seed's `IOContext` record registers its known-empty generic
parameter order alongside its fields, preserving `os_code: Int?` and
`cause: String?` at MIR joins rather than falling through to an integer guess.
Core `IOContext` construction narrows present OS codes exactly once through the
generic native optional-slot adapter, not a second record-specific conversion.
The Phi ownership and drop-compatibility guards remain unchanged.
The named-tuple registry fixture queries the real terminal signature metadata
instead of calling Core-private terminal members. The gate fixture assembler
isolates this Foundation registry witness and its canonical authority closure
from the other fixtures' entry points.
Ordered or-patterns stop at the first matching alternative; failed nested
carrier probes leave the subject available for subsequent alternatives. The
Rust reference now mirrors native Sema's recursive Result-carrier admission and
native codegen's ordered branch structure. Its owning payload projection takes
the carrier only when no later reachable MIR use needs it.
The call-coercion legality check distinguishes a plain Read trait view from
owning trait boxing, matching the checked call access. Borrowed non-Copy enum
payloads remain available after the call. Function/union conversions, authority
boundaries, and owning conversions retain the non-Copy consumption guard.

The existing TLS transport refusal runs before route lookup on both the
argument-validation and direct-call lowering paths. Pure TLS configuration
and certificate data remain available; transport produces the canonical
pending-decision finding, reported by the CLI as E2104. The pack-free
`run-lower-fixtures.mjs --tls-refusal` mode exercises both lowering paths.

Native runtime ownership is partitioned per C symbol, including members that
share a carrier. A Jet provider owns its physical ABI; a Core source operation
uses its checked body, not a similarly named Rust layout. Seed-only adapters
retain logical route metadata but are excluded from native-pack emission by
`cfg(not(jet_native_pack))`. Missing providers use the existing undefined-symbol
link refusal reported as E2104. The b5a331841 no-retained-Rust-Box invariant and
the pack producer's independent rejection of Rust symbols remain enforced.

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
  (ownership transfer preserving layout and drop glue; the source dies),
  `Reown` (consuming pointer-owner conversion preserving layout but selecting a
  new drop obligation), `Copy` (trivially destructible values only),
  `Call` (signature ID; callee a module function or a runtime symbol),
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
  calls, lock cmpxchg, xchg, pause, cld/std, and the string moves `rep
  movsb`/`rep movsq` (they write rcx, rsi and rdi; a template that sets the
  direction flag clears it again). Memory addresses admit a base register
  or named input with an optional signed-32-bit `+ displacement`.
  Callee-saved/frame registers, unsupported syntax, unbalanced stacks,
  misaligned calls, missing/duplicate return anchors, and named input reads
  after their bound register was written are registered native refusals:
  `LIRLoweringIssue` with the `inline asm:` prefix and the offending line.
  No bytes from a refused template enter the LIR. The OS-template witnesses
  in `Tests/Fixtures/InlineAsm.jet` use the actual `OS/Linux.jet` source.
  `inline-asm-exit` calls the raw seven-argument syscall leaf with Linux
  `exit_group(23)` and expects process exit 23 without stdout or stderr;
  together with the write/memory and call-address fixtures it covers the
  compiler's kernel and indirect-call leaf shapes.
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
`Move`, `Reown` or `Drop`, no `Copy` of a value with drop glue, Bool conditions, and
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

All lowering entry points share `lower_selected`'s explicit native-pack,
shared-site, import-closure and C-export modes. Preparation receives those
same modes; native imports select their actual render roots once. Function
entry initializes partial-drop live bits and the consuming Close receiver's
identity cell on both ordinary and split-entry paths. Ordinary boxed drop
uses the module's normal glue; receiver-specific structural cleanup stays
with the Close identity mechanism, not a type-wide drop override.

### Compiler-unit coverage measurement (S3)

`Compiler/JetBackend/Tests/self-host-rank.mjs <probe> [output-directory]`
consumes the self-host driver's `run/self.progress`, `run/self.issues` (or
incremental `self.partial-issues`), preparation issues and conversion metadata.
It also accepts a parent containing `slices/<package>/` from `run-slices.sh`.
All slices must come from the same MIR dump: descriptor names identify shared
functions, concrete instances and glue across slices.

The outputs are `RANKED.tsv` (conditional greedy blocker coverage plus package
totals), `OVERLAP.tsv`, `PACKAGES.tsv` (functions reached, locally lowered clean,
percentage clean), and `SLICES.tsv` (completion status per probe). Conversion
failures are included as blocked function evidence; global preparation failures
remain separately identified. Clean local lowering does not prove that callees,
object emission, runtime linking or compiler tests are clean.

Set `SELFHOST_GOLDEN_LOWER` to the matching golden run's `lower/` directory
(the S3 baseline is `nat-n7full-s0/lower`). Without golden issue artifacts,
overlap is explicitly unmeasured, never silently classified compiler-only.
The ranker refuses to report coverage when no lowering measurement exists.
A partial run never counts its unfinished last-started function as clean;
drivers may append `run/self.completed` names after flushing each function's
issues to provide precise completion evidence.

`self-host-slices.mjs <probe>` builds package root inventories from
`self-all.functions.tsv`, `self-all.sources.tsv` and `compiler.map.json`.
It writes `SLICE-ORDER.tsv` and `slice-order.txt`, ordered by root function
count (smallest first, package-name tie break). This is a scheduling heuristic,
not a claimed bound on reachable dependencies. The slice runner publishes the
aggregate table after each package so later slices do not hide early evidence.
`self-host-map.mjs` recovers the same map shape from `package-unit.mjs` source
markers, preserving UTF-8 byte offsets for package-scale measurements.

The isolated ranker test uses synthetic inputs, not compiler coverage:
`JET_SCRATCH_ROOT=<disk-path> node Compiler/JetBackend/Tests/self-host-rank.test.mjs`.


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

Primitive checked `String + String` calls `jet_rt_string_concat` with two
borrowed handles and receives a fresh owned String. The canonical runtime
sizes the output once; lowering does not concatenate bytes or allocate an
intermediate builder. `Fixtures/Strings.jet`'s `strings-concat` witness drops
the input owners before reading results, including empty, same-input,
Unicode and embedded-NUL cases.
The native `List.join` helper's owning `show<String>` callback also delegates
to the ordinary renderer. A borrowed rendering is cloned before returning
it, so releasing the callback output cannot consume the List's element.
`strings-join` covers multi-element Unicode, singleton and empty Lists,
dropping the Lists before reading the joined text.

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
| `Int`, ranges/tags/quantities over Int | exact-Int word | `i64` (Integer 8) | `jet_rt_int_drop` (inline tag fast path; BigInt releases one reference) |
| `Char`, `IntN(_, 64)` | word | `i64` | none |
| `Bool` | flag | `i8` (Integer 1, 0 or 1) | none |
| `F32` (constants/reader materialization/text only) | `Float32Bits`: zero-extended raw binary32 word in registers | four stored bytes, align 4 | none |
| `Unit`, `Never` | no LIR value | — | — |
| `String` | runtime handle to a C-layout `{ptr, len, cap}` UTF-8 header (byte counts, words at offsets 0/8/16 on x64) | `ptr` | `jet_rt_string_drop` |
| `RangeCursor`, `IterCursor` | runtime handle | `ptr` | `jet_loop_cursor_drop` |
| struct, tuple | inline aggregate: pointer to storage holding the stored fields at natural offsets (`Lower/Values.jet`) | `ptr` | generated glue on the storage (frees nothing) |
| enum, every variant without payload | discriminant word | `i64` | none |
| enum with a payload variant | heap box: discriminant word at 0, payload members at natural offsets from 8 | `ptr` | generated glue |
| `T?` | null for absent, else a box holding the value's bytes at 0 | `ptr` | generated glue (null-safe) |
| `Result<T, E>` | box: 1 (Ok) or 0 (Err) at 0, payload at 8 | `ptr` | generated glue |
| Single checked trait object | owned box `[concrete drop-helper ID, concrete payload @8]` (`Lower/Traits.jet`); payload alignment above 8 is a named refusal. Dispatch compares stable helper IDs against the closed implementation inventory, never arbitrary code addresses. Concrete payload keys are registered during MIR body lowering; ordinary-glue helpers cannot add keys. Deferred dispatchers are emitted exactly once after draining ordinary glue, including newly registered nested-payload helpers; an inventory-growth guard rejects a regression. Concrete-to-trait calls, slots, Lists, Options and declared results retain ordinary payload ownership. `Renderable` delegates to the payload's selected Show/Display renderer. | `ptr` | concrete payload clone/drop followed by shell release; copied receivers are retired even when the concrete method only borrows; Move methods transfer the payload and release the shell; unknown dispatch keys trap |
| `[T]` | box: `[len, cap, data]`, elements `max(8, size(T))` bytes apart | `ptr` | generated glue |
| `[K:V]` | the `[T]` header over `(key, value)` entry tuples in ascending key order (BTreeMap's); lookups are a binary search (`Lower/Maps.jet`); keys are exact Int, String or word-sized scalars | `ptr` | `[T]` glue over the entries |
| `Set<T>` | the ordered `Map<T, Unit>` entry header (`Lower/Sets.jet`); exact builtin nominal identity, binary-search membership, duplicate keys released, and copies have independent buffers. Set-to-List converts entry stride to packed element stride; values snapshots use the canonical native Iter boundary. Show/Display/Debug sort rendered strings lexically, matching HashSet's canonical deterministic rendering, not numeric key order. | `ptr` | `[T]` glue over `(key, Unit)` entries |
| `Rank<T>` | the same ordered `Map<T, Unit>` entry header as Set, for native Map-supported ordered key types. From/add/remove/has/clear/len/is_empty, first/last, to-List, union and intersection use that carrier; first/last and snapshots clone keys. Rendering visits numeric/key order directly without lexical text sorting or a temporary snapshot, selecting Show/Display/Debug with the same explicit helper-name dispatch as Set and the canonical renderer. Difference, symmetric difference and subset/superset/disjoint remain explicit native refusals. | `ptr` | `[T]` clone/drop glue over `(key, Unit)` entries |
| `Tally<T>` | ordered `Map<T,U64>` entry header for native Map-supported keys (including all-unit enums). New/add/remove/count/has/len/is_empty/clear preserve multiplicity; len sums counts, remove is absent-safe and erases only the final occurrence. `any` lends one key per nonzero distinct entry and short-circuits, using the existing plain-module closure lowering; stop-point modules retain their explicit refusal. Increment/decrement/count reuse one binary search without an Option box. No new rendering policy or runtime export. | `ptr` | independent entry-buffer clone/drop and structural count-sensitive equality |
| `Queue<T>` | owned ring `[len, cap, data, head]` (`Lower/Queues.jet`), packed List stride, constant-time end pushes/pops; growth linearizes live slots. Owned List construction transfers its buffer; borrowed construction snapshots it. Split moves the suffix; delete preserves logical order and moves the shorter side. | `ptr` | live ring elements only; clear keeps the buffer, drop frees it; clone produces an independent linear ring |
| `PriorityQueue<T>` | binary max-heap in an owned List header/buffer (`Lower/PriorityQueues.jet`), using the same exact/byte/derived member ordering as native Maps. Peek clones the maximum; pop transfers it and repairs the heap. Canonical remove slots and sorted views are highest-first, not physical heap positions. | `ptr` | ordinary List clone/drop; sorted rendering owns and retires its independent snapshot |
| `View<T>`, `ViewMut<T>` | the `[T]` header `[len, 0, data]` in its holder's storage, `data` at the window's first element of the owner's buffer (`Lower/Lists.jet` lower_view_element); capacity 0 marks a buffer it does not own; List reads read it unchanged; a materializing `Copy` into `[T]` clones the elements | `ptr` | none |
| `View<str>` | an owned String handle holding the window's text: the `_view` kernels run their owned twins (`jet_string_after`, `jet_string_before`, `jet_unicode_trim`); costs one allocation the zero-copy `&str` avoids, otherwise unobservable (a window is only read) | `ptr` | `jet_rt_string_drop` |
| `Distinct`/`Alias` types | as their base | — | — |

The `F32` carrier is deliberately not a numeric Integer despite its word
register layout. Reader routes return binary32 in the low 32 of their
zero-extended out word. Native storage writes four bytes; reads load U32 and
zero-extend, with no FP conversion. Display and Debug borrow that raw word
for `jet_rt_float32_to_string` and `jet_rt_float32_debug` respectively, using
binary32 shortest text rather than widening to binary64 text.
Local, aggregate-tree and static constants pack the MIR binary64 literal into
binary32 once with integer ties-to-even rounding, preserving signed zero,
subnormals, infinity and the high quiet-NaN payload. The constants fixture
covers local, record, List and global storage plus rounding boundary bits.
Numeric, comparison and cast operations remain explicit refusals in this
constant/reader/text scope. Future FP-register operations must use the canonical LIR
`Bitcast`, not introduce another reinterpretation mechanism.
F32 runtime result admission is a named allow-list: only
`jet_reader_read_f32_le` and `jet_reader_read_f32_be`, producing
`Result<F32, String>`. All F32 route parameters, F32-containing list/tuple
results and other producers receive explicit refusals until their canonical
Jet provider is approved; a generic Word carrier never grants admission.
Only `Reader.take`, `Reader.skip` and `Reader.seek` narrow their exact-Int
count/position operand through the existing checked `jet_int_owned_to_i64`
adapter. A count outside I64 stops before the method, matching reference
`int_value`; an in-range negative or out-of-bounds count still reaches Reader
and retains its ordinary Err text. The conversion borrows the operand and
does not broaden the Word ABI for other routes.

`Int.to_radix(base)` remains checked `String`. Its runtime descriptor keeps
the kernel's raw `Result<Text, Text>` ABI, and Lower supplies the canonical
caller adapter: success retains the owned String directly, with no Result
box; failure borrows the selected error text for `jet_rt_numeric_stop` at
the canonical MIR file and line from `lower_numeric_location`. The selected
payload matches MIRRust and JetEmit without weakening the checked result.
The valid/invalid radix Lower fixtures cover both edges without making
runtime execution claims.

Scan capture classification reuses the canonical `lower_packed_storage_type`
alias/tag resolver for fixed integers, then requires an `IntN` storage kind.
The shared word carrier therefore does not admit binary32 as an integer scan
capture. The existing scan refusal fixture also checks I64/U32 admission and
F32 refusal at this storage boundary.

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
Borrowing `jet_rt_string_concat` returns an independent owner with one exact
buffer allocation and capacity equal to the two byte lengths' sum. Empty
concatenation retains pointer 1/capacity 0; size overflow uses the existing text
capacity panic. Raw witnesses cover ASCII, multibyte UTF-8, empty sides, aliased
inputs, interior NUL, exact capacity, and allocator-class balance after dropping
the inputs before the result, plus the capacity-stop edge.
The raw `TextTests` library exposes `text_tests`, not an executable entry.
Its runner adds `run` in a sibling executable directory, outside the runtime
package manifest, before compiling the ownership and UTF-8 witnesses.
C foreign String arguments use a separate byte buffer with a trailing NUL;
the caller frees it with its length including that terminator. Unpacking
validates UTF-8 while finding the terminator and copies into an owned E4
header before argument buffers are freed, including interior-pointer returns.
Interior NUL, null results and invalid UTF-8 use the exact Rust panic messages
through the shared panic stop path; raw text witnesses cover all three stops.
The private `jet_string_validate_utf8` adapter uses that same scalar validator
over native U8 list slots (eight-byte stride), returning the checked
`Unit String!` tag and owned error text. Success allocates nothing. Invalid
subparts and incomplete tails preserve the strict decoder's exact error text
and byte index; the native corpus compares them against the Rust seed oracle.

`Bits` owns a sorted unique sequence of nonnegative native I64 indices in an
L5 `[len, cap, data]` header. Its eight public route exports retain their names,
but allocate no Rust handle. `count` is membership size; `len` is one past the
largest member (zero when empty), with the seed's native I64 overflow stop.
Public Word inputs borrow exact-Int carriers and convert through the shared
Int seam. `to_list` transfers fresh exact-Int slots through the route's out
pointer and returns a native length. Copy/clone and rendering never consume
the source; clear/drop free only this set's storage. All three render modes
match `Core/Collections.rs:175-187`: sorted list text with comma-space
separators (`[]` for empty). Constructor/clear use the native Bits providers;
no native set may be passed to a seed Rust handle function. Raw ownership
witnesses cover sparse len/count, negative rejection, duplicates, ordered
output, independent copy/clone, both Int seams, exact rendering, and balance.

Sizes: a word, handle or box pointer is 8 bytes (align 8), a Bool 1 byte, Unit
0. An inline aggregate is 8-aligned and padded to whole words, so copies move
words; a member sits at the next multiple of its alignment, and a recursive
(declared boxed) edge is one pointer to a wrapper with the present Option
layout. Box payloads and list elements use the same byte layout, so an inline
aggregate stored in one is its bytes. Boxes come from `jet_rt_alloc(size,
align)` and return to `jet_rt_free(ptr, size, align)` with the same size (one
size function per box kind serves construction and glue). A variant's
discriminant is its index. Unions use their structural enum's box layout.
Trait values use the owned payload layout below. Generic instances lower as
their concrete instances (above). Function values are closure boxes.

Native `Secret` owns a 24-byte, align-8 native List header `[len, cap, data]`.
Adoption clears the moved input header, borrowed byte access aliases the real
parent, and growth remains forbidden. Close wipes the entire capacity before
clearing the header and freeing the buffer; final drop frees the header once.
`RT_HOST_U8_STRIDE` in `JetRuntime/Source/Collections.jet` selects the shared
U8 element width, independently of buffer alignment. Wipe and free share the
same cap × the host U8 stride byte count, with buffer alignment 8. Changing
the canonical list epoch changes Secret's byte count with it; there is no
private Secret stride to drift from the actual List allocation.

Native `Secret` owns a 24-byte, align-8 native List header `[len, cap, data]`.
Adoption clears the moved input header, borrowed byte access aliases the real
parent, and growth remains forbidden. Close wipes the entire capacity before
clearing the header and freeing the buffer; final drop frees the header once.
`RT_SECRET_ELEMENT_STRIDE` in `JetRuntime/Source/Secret.jet` selects the landing
epoch's U8 element width (packed stride 1), not the buffer alignment.
Wipe and free share the same `cap * stride` byte count, with buffer alignment 8.
The packed-integer epoch must change the stride constant and its native
witness atomically; it must not change the post-E2 word-stride baseline.

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

A value's drop action comes from its MIR physical ownership fact:
`drop != NoDrop` and neither read nor write borrow. Source-language `Copy`
does not mean dropless: exact Int remains semantic `Copy`, with `drop = Value`;
fixed-width integers and Char remain `Copy/NoDrop`. Both compiler halves use
the same predicate in their ownership verification. Only a `Copy/NoDrop`
carrier can strip an implicit physical clone fact to a scalar copy.
MIR `Drop(value)` becomes LIR `Drop` when the value carries a drop action.

Foundation owns the physical-consumption classifier used by MIR Lint, legality
and last-use selection: `Yes(requires_owner)`, `Through(transfers)` and `No`.
`Through` preserves prospective last-use traversal without pretending that an
implicit clone or payload projection consumes its source. Call consumption
uses checked argument flags and checked receiver-first builtin/handle masks. Storage writes,
constructors and return/yield transfers use the same classifier. Phi inputs
transfer on their selected predecessor edge, not together at the destination;
`Break` is prospective only because its target Phi performs the transfer.
TaskGroup task operands and Spawn's closure transfer their owners; borrowed
handle receivers (for example Sender.send's receiver) do not transfer, while
false-mask payload arguments do. The checked consumption-route query is shared
by all callers; there is no runtime-symbol-name ownership inference.
Transparent carrier operations preserve the source's physical ownership fact,
including a borrow; numeric casts inspect their source, and an exact-Int
identity numeric cast clones an independent owner. SSA retirement must use
these facts rather than establishing a backend frame-owner ledger.
Join-crossing/maybe-live SSA owners must use canonical place storage and the
existing drop-flag/flagged-cleanup mechanism, before borrowed aliases form.
Dominating must-live roots retain plain Drop. No second conditional-lifetime
opcode, backend frame shadow, sweep or worker-only cleanup ledger is allowed.

MIR schema 7 represents that storage association with `MIRLocal.ssa_value`.
A present value names the existing SSA definition's cell, not a second owner:
the definition writes the cell, Rust emission omits the separate local
declaration, and native lowering associates both identities with the same slot.
Original SSA ownership and projection facts remain unchanged. Legality must
reject additional cell writers and require canonical live-flag updates on every
definition and selected consuming edge. Schema-7 observations require a compiler
that produces the field; old schema-6 dumps must not acquire a guessed `None`.

The storage association and physical facts are a prerequisite tranche;
retirement + flag-discipline validation pending. Native prep remains OFF.
Existing semantic-Copy cleanup registration remains unchanged in this tranche;
the new facts do not by themselves establish complete SSA lifetimes.

Post-last-use place-read refinement is also a Foundation fact, not a native
clone heuristic. A read without a physical-transfer user becomes ReadBorrow
only when a CFG may-written proof shows its owner remains stable through every
dependent use. Copy reads otherwise keep their independent owned snapshot:
replacing a binding cannot change an already-read exact Int. The proof follows
borrowed carriers and stored holders, treats owner transfers as invalidation,
and retains owned reads for uncertain callees/unsafe access. MIR Lint's
view.valid family checks the resulting borrowed reads and their derived uses.
By-value inline reads, member extraction and copies snapshot canonical extent
and alignment even when their bytes are dropless; NoDrop does not authorize a
mutable-storage alias. These dropless snapshots need no resource clone glue.
Real ReadBorrow/WriteBorrow results keep the parent's storage. Lint's borrowed
SSA/root associations and held-copy stale state are generation-specific, so a
fresh source definition cannot refresh an older holder or Phi alias.

Drop glue is one LIR function per box type, `drop.<canonical type key>`,
taking the box pointer and returning nothing (`LIRDrop.Function`). It drops
every owned member of the active shape (testing the discriminant for enums and
Results, and skipping null for Options), then frees the box. Glue for nested
boxes and inline members names further glue; the list grows while it is
generated.

Ordinary nominal drop selects the checked `Close.close` method for the concrete
receiver, including a generic container's instantiated member type. Mono reaches
that method even when no explicit call names it. Close implementations are indexed
once by nominal declaration during preparation and reused by layout/drop queries.
The consuming receiver parameter's own cleanup uses a separate structural role;
all other values of that type, including recursive children and locals in close(),
keep ordinary Close. A function-local identity cell is invalidated before the
original receiver's structural release or whole transfer, preventing allocator
address reuse from inheriting its role. Inline receiver rebinding uses the live
flag (not byte-storage pointer equality); the receiver MIR parameter is cached.
No cleanup mask is invented by either role: checked MIR owns the cleanup bracket.

For a moved Close-bearing field or payload, clearing bytes is not sufficient:
an inline zero value can still have an observable Close body. Function-local
live bits guard drop-rest on the moved path, expand only ancestors and leave
siblings on ordinary glue. Conditional take clears only on its executed arm;
field/root replacement drops the old guarded contents before resetting bits.
Whole-owner local rebinding copies bits with the owner, and repeated fresh
definitions reset them. These bits do not change any value's ABI layout. Whole
escape into an aggregate, callee or return is rejected if a Close-bearing path
can still be moved out (backward reaching definitions follow local bit transfers).

The Close Lower fixture uses an actually owned `Secret { id: Int, token: String }`
in both generic `ClosePair<Secret>` and concrete `CloseDirectPair`. After taking
the first field, its structural assertion finds the Struct's owner storage by
the MIR pair local's checked type identity (not a byte-size heuristic), follows
O0 LIR control flow and private live-bit reset/clear stores: the owner's
first-field ordinary Close glue
must be unreachable, while the remaining sibling and the taken value each have
one reachable ordinary Close call. The `27\n28\n` stdout expectation is separate
from this structural assertion and requires an admitted runtime execution.

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
| `Copy` | the same value for dropless word, flag and tag carriers; `jet_rt_int_clone` for owned exact Int (packed tag fast path, BigInt retains one reference); `jet_rt_string_clone` for a String; clone glue for boxes; a fresh `StackSlot` filled by clone glue for an owned inline aggregate (a borrowing copy names the source) |
| `Move`, `AttachTag`, transparent `Convert` | the same value |
| exact-Int `+ - *` (Prelude route, arity 2) | inline test of each operand (`Overflows(Add, x, x)`: doubling an inline word does not overflow; `add; jo slow`), native op (Mul also checks `Overflows`), the same inline test of the result, else the route's runtime symbol in a cold block laid out after the hot code; joined in a block parameter |
| exact-Int other ops (division family, pow, shifts, bit ops) | call of the route's runtime symbol with `(left, right)`, plus `(file_ptr, file_len, line)` for routes of arity 4 |
| exact-Int comparison (Primitive) | native `Compare` when both are inline, else `jet_int_compare(l, r)` compared with 0 |
| word `+ - *` (Primitive) | `Binary`; with `overflow: Trap`, an `Overflows` test branches to `jet_rt_panic_overflow` first |
| word `/ %` (Primitive) | zero check to `jet_rt_panic_division_by_zero`, then a minimum-divided-by-minus-one check to `jet_rt_panic_overflow` (x86-64 `idiv` faults on it), then `Div`/`Rem`; signed I64 division truncates toward zero and remainder keeps the dividend's sign (`numeric-i64-truncation` fixture distinguishes this from floor division) |
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

Operations without a complete lowering report an issue by construct, rather
than supplying partial machine code.

### Closures

A function value is a pointer to a heap box `[reference count, code
address, release helper, captures...]` (Lower/Closures.jet). The code takes
the box as a hidden first parameter. A closure body (a function with capture
parameters) is the code; any other function used as a value runs through a
generated thunk that ignores the box. Clone glue counts a reference; drop
glue runs the release helper on the last one, which drops owned captures and
frees the box. A capture parameter that borrows (ReadBorrow, WriteBorrow or
Write access) keeps a pointer field in the body ABI. A `Place` operand stores
the outer place's address and never transfers its owner. A `Value` operand
owns its captured cell even when the body borrows or writes it: its bytes are
appended inside the same box and the pointer field names that internal cell.
Release helpers are specialized by operand shape, so they drop exactly the
owned cells and free the corresponding allocation, without extra allocations
or a per-invocation ownership test. Captured places are memory cells: in the
body the box field or referenced storage, in the creating function frame
storage every read and write goes through, so both see one value. On x86-64
`FunctionAddress` is `lea [rip + disp32]` and an indirect call is `call r10`.

`scope.guard` consumes a closure box without changing its bits. Its drop
obligation changes from releasing a closure to invoking its callback once and
then releasing it. Lowering expresses this as pointer-only `Reown`, not
`Move`: ordinary `Move` must preserve drop glue. `Reown` ends the source's
lifetime and requires an owning destination; optimizers retain its ownership
boundary rather than aliasing the differently typed owner away. The x86-64
allocator can share the dead source's home, avoiding an extra move.

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
The `para-map-owned` and `try-map-owned-{ok,err,empty}` MIR fixtures cover
owned String outputs surviving input retirement, the successful and empty
Result paths, and first-error short-circuiting with an owned produced prefix.

Parallel predicates evaluate through the same joined range walk, then move
source items to their source-ordered filter/partition destinations.
`para_fold` lends seed, step and merge closures to the canonical chunk
scheduler and merge tree: each chunk invokes a fresh seed and accumulates
locally, and merge helpers consume both partials. An empty input invokes
the seed once. The raw runtime bridge only schedules and transports owned
accumulator slots; typed reads, writes and releases stay in generated helpers.

`CarrierFact` borrows a Result's error report and deep-copies the checked
stored field into the returned partial or notes. A successful Result
produces an absent partial or an empty notes List without reading the error
payload. Host-borrow callback adapters preserve the native callable box;
the consuming host adapter still owns its checked argument ABI.

`ReflectOf` builds the canonical runtime Value/Field records from checked
type names, paths, display and field rows. Field displays use universal Show;
they retain a typed Value with no nested field rows, as the canonical
`from_field` constructor does. The result crosses back to native record
storage when MIR supplies the registered record shape. Reachability roots
the selected Show implementation of every reflected field.


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
| `jet_rt_int_clone`, `jet_rt_int_drop` | `(exact Int) -> exact Int`, `(exact Int)`; clone borrows, drop consumes one owner |
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
canonical route C symbol. The runtime sources' `#Export(c)` declarations are
the export set; `gen-c-abi-routes.mjs` keeps the shared lowering metadata but
omits the Rust adapter body of every such declaration. The generators read the
runtime-only `sources.list`; `gen-runtime-lifecycle.mjs` derives
`lifecycle.list` from `// runtime-init|fini: N` comments. The route generator
checks the Jet declaration against the derived C ABI, including every
flattened list argument and result out-slot, and fails rather than silently
skipping a Jet-owned route whose ABI cannot be derived. Exact `Int`, typed
`String`, write borrows, failures, and mismatched parameter/result widths are
not raw route signatures. `--check` validates signatures as well as
generated-file freshness.
Native source-identity descriptors (`lower_native_type`, `lower_native_export`)
and logical-environment descriptors (`lower_environment_route`) use the same
canonical emitter and declaration-proven inputs. Capture them only from a
fresh all-parts emission matching the current compiler/Core/provider pin;
historical descriptor snapshots are not replacement table data. At Rust
cutover, retain the populated tables as canonical Jet source with generator
and input-digest provenance, rather than retaining the Rust-driven emitter.
`node Tools/agent/gen-runtime.mjs <allparts.rs> --check` checks trap literals,
the lifecycle list, runtime stop exports, and routes in one keep-going invocation. Without `--check`
it regenerates them in dependency order. Narrow worker and held-job checks
can use `Tools/agent/lane-check.sh --runtime-generators <allparts.rs>` through
the cheap-check lane; this mode does not invoke Cargo.
The stop-export generator also accepts explicit `--write` and `--check`.
Its indexed registry-row parser materializes String `split`'s `Iter<String>`
with `.to_list()`, as does the handwritten Prelude-effect row parser.
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

The Jet runtime-only source manifest is assembled by the runtime-object path,
never the compiler bootstrap unit. Adjacent `// runtime-init: N` and
`// runtime-fini: N` comments on `#Export(c)` declarations produce explicit
`symbol init N` / `symbol fini N` export-list rows. Lifecycle functions take
no arguments and return nothing; the object writer emits the corresponding
priority-suffixed ELF init/fini arrays, without special-casing function names.
Allocator initialization has priority zero; host stdio initializes after it.
Equal priorities use lexical export-symbol order, matching the canonical
exports list rather than source declarations. Same-priority initializers
must be independent and idempotent; an initializer with a dependency uses
a later priority. Programs and entryless libraries consume this one inventory.

D-STDOUT-BUFFER1=A gives stdout one locked 8192-byte process-wide buffer:
newline flushing on terminals, full-buffer flushing on files and pipes.
stderr drains stdout first and writes immediately. The shared
`jet_rt_stdout_flush` hook is also a fini entry and is called at input,
blocking, child-launch, error-report and explicit-exit boundaries. Print
borrows the E4 text and appends its newline without allocating a second frame.
IOError crosses host C boundaries as seven eight-byte fields: kind, operation,
nullable owned resource text, OS-code presence, OS code, nullable owned cause
text, and resource-limit tag. Tags are generated from the canonical Core enum
registry; no Rust enum or String layout is inspected.
IOContext uses a 40-byte owner: operation, nullable resource text, OS-code
presence, raw i64 OS code, nullable cause text. It is the contiguous payload
at IOError+8; variant getters deep-clone it, while constructors consume its
header and transfer the text owners. The OS-code projection encodes exact Int
through the canonical numeric seam, never by treating a boxed Int as i64.
Core stdio wrappers check `IO.Write`/`IO.Flush` faults before entering the
raw transport. Runtime print, panic and abort do not consult those switches.
stderr text lines use a borrowed text iovec and a stack-local LF, avoiding a
temporary line allocation; byte lists compact eight-byte slots into bounded
mapping-owned scratch. A zero write keeps a distinct WriteZero sentinel,
not a fabricated errno. OS causes use pinned musl 1.2.5 wording from
`JetRuntime/Assets/MuslErrno1_2_5`, with source hashes and MIT attribution.

The native main wrapper captures argc/argv/envp through the explicit
`jet_rt_host_arguments` entry before user code. Runtime-private module globals
are the sole raw startup snapshot, not foreign data imports or a second
environment map. Entryless hosts must call that entry explicitly before
argument/environment access. Core owns the once-published logical Shared
environment and all subsequent writes; raw startup bytes remain immutable.
Entryless hosts lend environment arrays only during capture: valid entries
are copied once into a single runtime-owned NUL-terminated byte block and the
index points into it. Later host `setenv`/`putenv` cannot invalidate the
snapshot. argv remains borrowed for the runtime lifetime. Zero-sized process
stream tokens are the constant non-null word 1 and have no allocation/drop.
Startup indexing follows Rust 1.90 `sys/env/unix.rs`: skip empty/malformed
entries and search `=` after byte zero, preserving names beginning with `=`,
non-UTF-8 bytes, duplicate rows and host order. Core transfers use owned
List<U8> headers with eight-byte slots. Runtime-only scheduler/deadline flags
read this immutable snapshot through an internal helper; Core overlay writes
do not alter those flags. No overlay-provider callback is registered.

Network addresses have explicit C layouts, independent of Rust enums:
IPAddr is a 24-byte family/octet carrier; SocketAddr is 56 bytes with the
IPAddr inline, followed by port, IPv6 flow/scope and a zero reserved word.
Resolver transfers are inline arrays of those 56-byte rows, in platform
order without sorting or deduplication. `__ResolveError` is a 32-byte owner
with exact-Int kind/code words and an owned E4 message. Its projections and
clone/drop use the canonical numeric ownership seam.
TCP and Unix stream holders are 56 bytes: the FD72 pointer, one reserved
word, separate shutdown flags, read/write timeout milliseconds, and a
metadata mutex. They do not keep another closed flag. TCP listener copies
duplicate the descriptor into a new FD72; Unix listener copies retain the
shared FD72, matching the Rust Arc lifetime. UDP's eight-byte holder points
to one shared 32-byte state containing refs, mutex, FD72 and a single timeout
used for both directions. All timeout projections transfer canonical exact
Int owners, while the source alone validates durations and chooses deadlines.
The resolver uses platform `getaddrinfo` with zeroed hints except SOCK_STREAM
and a null service, then applies the port. All returned chains are released
with `freeaddrinfo`, including empty results. EAI_SYSTEM preserves errno;
other failures preserve the platform message prefixed as Rust's lookup does.
The lookup worker holds its own reference to the input/output job, so a
deadline or cancellation can abandon a lookup without joining or freeing data
still borrowed by libc. The musl imports are the ledgered interim platform
floor, not a hosts-only fallback; hosted CLI cutover requires glibc dynamic
linking under D-NETDNS2 and D-OS-FLOOR1.

Native socket waits share one epoll worker and scheduler ParkSlots. Kernel
events carry monotonically allocated IDs, resolved under the poller mutex;
they never carry pointers to wait nodes that may have been freed. Each waiter
retains the FD72 state until it has been deregistered, unlinked, and dropped.
Explicit close unregisters the descriptor and wakes every waiter as closed,
then defers physical close while network references remain, preventing fd
reuse from retargeting a parked operation. Waits hold no runtime mutex across
scheduler yield, and do not allocate a worker thread or duplicate a descriptor
per operation.
Raw socket syscalls are nonblocking single attempts over the canonical FD72
owner. Socket creation and accept atomically set CLOEXEC and NONBLOCK. The
existing host mapping lends bounded address/option scratch under its mutex,
after the stdout buffer, so address conversion adds no temporary allocation.
Core distinguishes EINPROGRESS from completion and consults SO_ERROR after
writable readiness; the raw connect leaf never reports it as success.
Network close marks the descriptor state closed and unregisters waits, while
the socket owner retains the physical descriptor until final drop. This
matches the Rust TCP/UDP logical-close lifetime and preserves descriptor
ownership even when an operation has already observed closed.
Stream reads and writes make one nonblocking attempt; source alone chooses
retry and wait behavior. Socket writes use MSG_NOSIGNAL, independently of the
embedding host's signal disposition. UDP receives expose the original
datagram length through MSG_TRUNC while initializing at most the borrowed
buffer length; Core constructs the packet and truncation fields.
Unix path refusal statuses are distinct from OS errno: -4098 means interior
NUL and -4097 means too long for sun_path. The leaf checks NUL first, then the
108-byte bound, matching Rust 1.90 `os/unix/net/addr.rs` sockaddr_un. Core maps
those facts to the exact std InvalidInput messages with no OS code. An empty
path preserves std's two-byte address length rather than inventing a name.

Raw clock samples carry exact Int words, not truncated i64 timestamps. Wall
samples preserve the unsigned duration magnitude and an independent
before-epoch flag, including normalized negative POSIX timespecs. Host
monotonic samples use one lazily published process-relative origin, mirroring
Rust's `TimeMonotonic.rs` static `OnceLock<Instant>`, rather than substituting
the scheduler's absolute monotonic clock. Clock syscalls borrow the calling
thread's existing timespec scratch and read both words before any other call
can reuse it. Core alone selects world, replay or host samples and clamps
the public Instant scalar.
The reserved Core Clock `__clock_mono_ns` seam samples that same host epoch
without controlled, world or replay selection; it preserves the existing
hidden Clock implementation, not a new user-callable pure clock operation.
Controlled monotonic callbacks live in the existing dynamic TLS registry;
the clock root owns just the registry key, not another thread-context slot.
Private TUI capability inputs sample the real stdout tty and owned immutable
startup environment values through two narrow host-fact exports. TCGETS
continues to borrow host scratch only while the host mutex is held.
Time-zone environment reads lend the immutable startup snapshot and decode
invalid UTF-8 lossily, matching `var_os(...).to_string_lossy()` independently
of the logical environment overlay. File-zone reads transfer owned native
byte lists; native embedded-zone absence is the platform seed's actual fact
(the embedded TZif table is a wasm fact), not a file-system fallback.

The Jet runtime-only source manifest is assembled by the runtime-object path,
never the compiler bootstrap unit. Adjacent `// runtime-init: N` and
`// runtime-fini: N` comments on `#Export(c)` declarations produce explicit
`symbol init N` / `symbol fini N` export-list rows. Lifecycle functions take
no arguments and return nothing; the object writer emits the corresponding
priority-suffixed ELF init/fini arrays, without special-casing function names.
Allocator initialization has priority zero; host stdio initializes after it.
Equal priorities use lexical export-symbol order, matching the canonical
exports list rather than source declarations. Same-priority initializers
must be independent and idempotent; an initializer with a dependency uses
a later priority. Programs and entryless libraries consume this one inventory.

D-STDOUT-BUFFER1=A gives stdout one locked 8192-byte process-wide buffer:
newline flushing on terminals, full-buffer flushing on files and pipes.
stderr drains stdout first and writes immediately. The shared
`jet_rt_stdout_flush` hook is also a fini entry and is called at input,
blocking, child-launch, error-report and explicit-exit boundaries. Print
borrows the E4 text and appends its newline without allocating a second frame.
IOError crosses host C boundaries as seven eight-byte fields: kind, operation,
nullable owned resource text, OS-code presence, OS code, nullable owned cause
text, and resource-limit tag. Tags are generated from the canonical Core enum
registry; no Rust enum or String layout is inspected.
IOContext uses a 40-byte owner: operation, nullable resource text, OS-code
presence, raw i64 OS code, nullable cause text. It is the contiguous payload
at IOError+8; variant getters deep-clone it, while constructors consume its
header and transfer the text owners. The OS-code projection encodes exact Int
through the canonical numeric seam, never by treating a boxed Int as i64.
Core stdio wrappers check `IO.Write`/`IO.Flush` faults before entering the
raw transport. Runtime print, panic and abort do not consult those switches.
stderr text lines use a borrowed text iovec and a stack-local LF, avoiding a
temporary line allocation; byte lists lend packed bytes directly without
scratch compaction. A zero write keeps a distinct WriteZero sentinel,
not a fabricated errno. OS causes use pinned musl 1.2.5 wording from
`JetRuntime/Assets/MuslErrno1_2_5`, with source hashes and MIT attribution.

The native main wrapper captures argc/argv/envp through the explicit
`jet_rt_host_arguments` entry before user code. Runtime-private module globals
are the sole raw startup snapshot, not foreign data imports or a second
environment map. Entryless hosts must call that entry explicitly before
argument/environment access. Core owns the once-published logical Shared
environment and all subsequent writes; raw startup bytes remain immutable.
Entryless hosts lend environment arrays only during capture: valid entries
are copied once into a single runtime-owned NUL-terminated byte block and the
index points into it. Later host `setenv`/`putenv` cannot invalidate the
snapshot. argv remains borrowed for the runtime lifetime. Zero-sized process
stream tokens are the constant non-null word 1 and have no allocation/drop.
Program mains separately call `jet_rt_host_program_start` after startup capture
to ignore SIGPIPE, matching Rust's program lifecycle. This is not an init-array
entry: entryless libraries preserve the embedding host's signal disposition.
An explicit program-start opt-in fails fatally if the signal action is refused.
Startup indexing follows Rust 1.90 `sys/env/unix.rs`: skip empty/malformed
entries and search `=` after byte zero, preserving names beginning with `=`,
non-UTF-8 bytes, duplicate rows and host order. Core transfers use owned
List<U8> headers with packed bytes and allocator alignment 8. Runtime-only scheduler/deadline flags
read this immutable snapshot through an internal helper; Core overlay writes
do not alter those flags. No overlay-provider callback is registered.

Raw clock samples carry exact Int words, not truncated i64 timestamps. Wall
samples preserve the unsigned duration magnitude and an independent
before-epoch flag, including normalized negative POSIX timespecs. Host
monotonic samples use one lazily published process-relative origin, mirroring
Rust's `TimeMonotonic.rs` static `OnceLock<Instant>`, rather than substituting
the scheduler's absolute monotonic clock. Clock syscalls borrow the calling
thread's existing timespec scratch and read both words before any other call
can reuse it. Core alone selects world, replay or host samples and clamps
the public Instant scalar.
The reserved Core Clock `__clock_mono_ns` seam samples that same host epoch
without controlled, world or replay selection; it preserves the existing
hidden Clock implementation, not a new user-callable pure clock operation.
Controlled monotonic callbacks live in the existing dynamic TLS registry;
the clock root owns just the registry key, not another thread-context slot.
Private TUI capability inputs sample the real stdout tty and owned immutable
startup environment values through two narrow host-fact exports. TCGETS
continues to borrow host scratch only while the host mutex is held.
Time-zone environment reads lend the immutable startup snapshot and decode
invalid UTF-8 lossily, matching `var_os(...).to_string_lossy()` independently
of the logical environment overlay. File-zone reads transfer owned native
byte lists; native embedded-zone absence is the platform seed's actual fact
(the embedded TZif table is a wasm fact), not a file-system fallback.

A Jet type that generated Rust holds as a native runtime type crosses as an owned
`Box<T>` handle. The generator reads Context.rs and predicates.rs name tables and the
explicit names in MIRRust `rust_apply_type`, using the same canonical-Core probe as
MIRRust. Each such type gets `jet_rt_handle_{drop,clone,display,show,debug,eq}_<snake(T)>`
adapters where the runtime type supports the operation. A `&mut T` kernel parameter
takes the caller's handle as `HandleMut`, and the kernel mutates the value inside that
box. A tuple result writes each logical item through its flattened carrier
out pointers: a scalar needs one; an Option needs a presence tag followed
by its payload slots. Nested Options retain each tag, preserving None,
Some(None) and Some(false) as distinct values.
Explicit MIR Write arguments must match `HandleMut`; ordinary read and move arguments
must not. Canonical Core identities override a source layout only when the generated-Rust
name rules replace that exact identity; source-owned enums and records retain their
native constructors and matches. Tuple adapters write zeroed word slots, and lowering
copies each item into the tuple's naturally aligned native field offset.
The route generator's tuple/Option regression compiles and executes the
production-generated adapter body and checks the walk/terminal route inventory.
Parameters whose runtime signature is `i64`, including borrowed parameters and
list elements, narrow the exact-Int carrier through the existing `native`
conversion rather than interpreting a big-integer owner as a machine value.

The vetted raw Object view projects the native Object entries into
`[len, 0, data]` View storage, retaining the source buffer and its loan rather
than cloning a runtime tree. The owned entries constructor transfers the
original List header into the Object payload with ordinary native ownership.
Zeroize lends the original Write List<U8> data/length to the existing native
provider; it never wipes a temporary Rust Vec. `NativeValues.jet` lowers and
lints all three callable witnesses in the route-carrier fixture harness and
rejects cloning-bridge imports.

Carrier admission unwraps `carrier_of(checked_type)` into a fresh `actual`
and then compares `actual == requested_carrier`. A dotted `.Val(carrier)`
or bare `Val(carrier)` on the right of `==` is a binding pattern in the
host's pattern position, not a comparison with that existing carrier, and
would admit every `Val` unchecked. The same explicit unwrap-and-compare
rule protects mutable handle fields, record sequence kinds and consuming
Close receiver slot identities. The route fixture covers distinct
Bool/Flag, Int/Word and String/Text carriers, List element identities and
composed Option payload identities, in addition to the deliberately
wrong Bool-typed Int route whose callable entry must be refused.

`IOError` is a C-carrier boundary, not a `Box` of the Rust enum. Its 56-byte,
eight-aligned `JetCIOError` stores kind, operation, optional E4 resource text,
OS-code presence and value, optional E4 cause text, and resource-limit tag.
The generator matches the resolved Core error declaration and uses
`error_handle`/`error_view`/`error_take` when crossing the remaining Rust
seed kernels, including nested records and Option/Result/List payloads.
`IOContext` uses a 40-byte, eight-aligned `JetCIOContext` with operation,
resource E4 pointer, OS-code presence/value, and cause E4 pointer; it matches
bytes 8–48 of `JetCIOError`. Its remaining seed-kernel boundaries use
`context_handle`/`context_view`/`context_take` rather than Rust record addresses.
The private resolver error uses the same resolved-record mechanism once L7
owns its canonical helpers: `JetCResolveError` is 32 bytes (owned exact-Int
kind, OS-code flag word, owned exact-Int OS code, owned E4 message). Both Int
words retain/release through the canonical numeric carrier; neither is
reinterpreted as a fixed integer. Its schema is read from the actual named
runtime declaration, including Int/Bool/Int/String field kinds.
Once the L8 task layer owns canonical clone/drop, `JetTaskFailure` uses a
16-byte, eight-aligned `JetCTaskFailure` (canonical enum tag, nullable owned
E4 reason) and `failure_handle`/`failure_view`/`failure_take`. Until that
coordinated cutover the task record retains its seed representation.
Every bridged carrier requires Jet-owned canonical clone/drop before
route generation. Hand context/error record exports are seed-only under
`cfg(not(jet_native_pack))` and use these same C headers. Context constructors
transfer child owners and free only the 40-byte header; projections deep-clone
E4 text directly, without an intermediate Rust String. Native packs contain
only the Jet definitions. These seed/verification bodies die at cutover.
Jet owns clone/drop and the E4 text children. Generated lifecycle, record,
and field exports use the same ownership filter and ABI checks as routes.
Mutable Rust-enum borrows and address-lending error fields/sequences are
refused with their route or record name; a Rust enum address is never treated
as the C header. Lifecycle source markers retain their init/fini priority in
the shared export list.
The unified generator runs host-error and host-carrier generators before
export and route generation, so their outputs participate in `--check`.
CString helpers called directly by `Lower/CForeign.jet` have a checked
lowering-only ABI table in `runtime-route-abi.mjs`; they are not invented Rust
routes. Packing borrows an E4 header and returns an owned NUL buffer; unpacking
returns an owned E4 header; freeing takes pointer and size including NUL.
For Jet-owned routes, generated `LowerRoute.native_lists` is true; it admits
nested List carriers (a List of Lists or of Text crosses as native headers
and buffers). Lists of Core records cross every route, Jet-owned or seed, as
a buffer of eight-byte record handles that the lowering rebuilds natively
(`list.record_handles` / `lower_record_list_from_runtime`), never as inline
rows: the runtime record shape (nullable OptText words, sequence owners,
DirEntry24, FieldError16) is not the native inline row, so a provider cannot
write one. A List of opaque handles (`[SocketAddr]`: JetSocketAddr boxes) is
a buffer of those handle words. `jet_std_fs_list_dir_raw`, `jet_dyn_field`,
the codec failure lists and `message_parts` build or read handle buffers;
`jet_net_raw_resolve_ordered` publishes Socket56 boxes behind handle words
(its inline-row worker output stays internal to the HTTP build resolver).
Both use the same data-pointer/length C call shape, and record schemas remain
checked; a native fixture reads a field of a route-returned record List so a
shape drift fails as a wrong value, not as a pointer-sized allocation.
StreamCursor's native cutover is one complete export family: all Cursor/Reader
routes, clone/drop, and the borrowed Cursor buffer projection are Jet-owned.
G1 rejects a partial family before generating routes; it retains logical
List/handle metadata while checking Reader constructors' explicit source stride
and Reader.take's target stride before its three result out slots. These
physical strides come from checked native element layouts (one or eight
bytes). Fixed-width Reader results are raw sign/zero-extended words, including
all unsigned 64-bit patterns; only Int-typed results use exact-Int ownership.
F32 reads preserve the low 32 IEEE bits in a zero-extended word, not a widened
F64 value. The Rust Core implementation remains a verification path, but no
stream Rust C adapter or Box lifecycle remains in the native pack.
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
Qualified Core record nominals resolve through their checked MIR declaration
identity to the runtime's short-name row, then must match the stored-member
schema. No suffix-only match authorizes a user type. Thus `core.files.list_dir`
rebuilds native, qualified `DirEntry` list elements rather than refusing the
runtime's `DirEntry` handle carrier; the qualified-record fixtures also retain
user-nominal and changed-schema refusals.
Generic record schemas bind the declaration's ordered type parameters to the
concrete arguments at a route boundary, then substitute every stored member
type before choosing its carrier. A concrete application's handle/schema key
includes the resolved arguments (UTF-8 hex suffixes on the runtime name), so
different applications cannot reuse one schema. Argument-free records keep
their existing keys and exports. Native lookup uses the corresponding concrete
Jet application name and the existing MIR declaration instantiation, not a
type-specific codec. Unresolved, inaccessible, gated or unsupported arguments
are generator errors naming the application. Nested declaration arguments are
explicitly refused; generic function monomorphization is not an ABI schema.
Source-owned generic records resolve through the emitters' existing
`Syntax::generated_path` identity only when the emitted runtime actually
declares that type. Concrete uses in non-generic emitted Core function
signatures supply arguments even when no C route mentions the record.
Schemas keep source member names; Rust projections retain the emitted
`__jet_` member names. Root-private fields remain private in Jet and in
emitted Rust: the internal adapter child module already reaches its parent
record without adding an accessor or widening user-field visibility.
If a runtime declaration already has concrete generic rows, an unsupported or
unregistered application is a named `LowerRepr.Unsupported`, never a silent
native-struct fallback. Generic Core enums use the same concrete handle key.
Ordinary Jet generics with no runtime declaration rows remain native.
Native glue metadata records the actual Core declaration and its ordered type
arguments, not a Rust layout. `lower_native_source_type` is the shared ownership
gate: the current MIR must contain that declaration and Lower must already
represent it as inline storage, a native payload box, or a native enum tag.
Opaque/special runtime handles are not admitted by declaration presence alone;
existing handwritten Jet C providers are excluded from generated descriptors.
Only surviving boundary callers need synthesized native glue. Direct native
reads/writes require no replacement C symbol.
Candidate admission also requires the actual emitted source nominal identity,
not a legacy Rust carrier with a similarly named Core marker. Nongeneric
nominals require a source-owned rule or an actual typed source reference;
unused marker declarations do not qualify. Inventory classification reports
source record/opaque candidates separately and labels native layout as
unexercised until the MIR predicate runs. Collection-holder symbols have no
invented declaration owner.
Recursive record collection members use generated `jet_rt_record_seq_*`
holders while crossing the boundary. Byte lists, record lists and `(key:
String, value: Record)` entry lists retain their native List layouts; the
holders expose only raw length, indexed read and capacity-reserved append.
`DataTree.Object` remains an insertion-ordered entry list, including duplicate
keys. `DataEntriesToMap` copies borrowed entries into the ordinary native Map
in source order, so the last duplicate wins without changing the original
DataTree. Record getters lend collection and nested-element storage while
rebuilding native values; constructors consume the freshly built holders.
The vetted `encoding.__bytes_view` leaf projects `DataTree.Bytes` directly
from its checked native enum payload into a borrowed `[len, 0, data]` header.
It allocates nothing and copies no byte elements; native U8 elements retain
their word stride. Parameter-zero provenance keeps the owner borrowed until
the view dies. Dropping a view never frees the owner's payload.

The route generator discovers declarations produced by invoked non-repeating
runtime macros, retaining their invocation's module and platform guards.
The all-parts seed includes HTTP and database route-demand bodies so importing
their modules cannot silently omit the host bridge instantiations. A source
registry ownership change regenerates both adapter and carrier tables together;
for example, `core.time.now` uses the DateTime-bearing `jet_time_now_utc` route,
not the former millisecond integer route.

`Lower/Codecs.jet` reaches the checked Encode/Decode implementation for the
concrete receiver and invokes its ordinary monomorphic Jet method. Finite
scalar specializations marshal into the existing canonical runtime traits;
they do not duplicate codec acceptance or diagnostics. Typed JSON/TOML/YAML
parse-to-tree specializations use the existing typed intrinsic's DataTree
instantiation before native Decode dispatch, preserving its error envelope.
`DecodeUnder` prefixes failures through the canonical FieldError helper and
leaves successful payloads intact. Unsupported container/callback carrier
edges remain explicit refusals rather than a second codec policy.

Typed CSV supplies a checked native Decode callback to the existing canonical
`jet_enc_csv_decode_rows` kernel. The private generated `jet_rt_codec_csv`
boundary lends each runtime DataTree, uses the existing NativeMeta/NativeValue
container slots, and transfers owned decoded rows or owned FieldError handles.
The borrowed callback's OCB1 consumed mask is zero. A native delivered-stop gate
leaves all outs unwritten; `jet_native_callback_body` resumes the canonical stop
only after the native callback has returned, before inspecting its tag or
initializing a NativeValue. Header projection, row order, error accumulation
and row-path framing remain in the canonical kernel, not native lowering.
This generated Rust adapter is a tracked temporary directive-19 leaf, removed
when the checked Jet-owned CSV intrinsic and native DataTree/FieldError path
replace the canonical Rust row kernel. It is not a permanent provider exemption.
The callback marshals the borrowed tree through `record.in` without allocating
a Rust tree clone. Native-freed row and handle-array buffers use `jet_rt_alloc`
and `jet_rt_free` with the same byte count and eight-byte alignment as lowering.
Opaque runtime FieldError boxes retain their matching runtime handle-drop
provider; they are converted to native records, never freed as native records.
For every supported row type, `lower_lazy_meta` stores exactly
`lower_list_stride(p, target)` in descriptor word zero. CSV checks its success
List's element is that same checked `target`; `lower_helper_list_drop` uses the
same `lower_list_stride` for all elements (including records), and frees
`cap * stride` at alignment eight. Empty callback error arrays are null:
`list.record_handles` calls `lower_list_buffer(length, 8)`, whose zero-length
branch returns null; the runtime bridge asserts that invariant before accepting
an empty failure vector.






Lazy collection routes use the existing `Collections.rs` and `LoopCursor.rs`
kernels through `CAbiIter.rs`; lowering does not recreate their iteration or
failure policy. A copied descriptor tree carries stride, clone/drop and checked
trait callbacks plus recursive element/error descriptors. Slots up to three words
are stored inline; a borrowed cursor projection retains the buffered owner until
advance, while the native cursor's taken flag prevents returning it twice.
File and stdin line loops use the same descriptor and cursor ABI. Their adapters
delegate source creation to `CollectionIoSources.rs`: an owned reader is lazy
and moves into the cursor, while a borrowed reader supplies the existing
snapshot. Only each yielded String's slot is marshaled; stride validation,
lookahead, early-exit release and source-error policy remain in the shared
kernels. Lowering rejects a line-source kind whose checked runtime carrier
does not match the reader, rather than reinterpreting another handle.

`Queue<T>` is a shared-runtime `VecDeque<NativeValue>` handle. Queue methods
marshal the checked element slots and invoke the existing collection kernels,
including front/back operations, borrowed peeks, deletion, split and join.
Owned inputs transfer their slots; borrowed inputs clone only when the kernel
takes ownership. Lookup equality comes from the checked needle descriptor,
and join registers only the element's Printable bridge. Queue cloning and
conversion to List clone elements rather than aliasing the queue's owners.
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
all-parts native runtime crate. Native-only owner releases (`jet_rt_int_drop`)
live in `CAbiIter.rs` for the same reason: the shared generated prefix gains no
unsafe for the native backend.

`Shared<T>` construction passes three native words: initialized value-slot
address, native descriptor, and the descriptor's monomorphic address-based
`lazy.drop` callback (zero for trivial `T`). The native cell stores this callback
separately at offset 80 and invokes it once, outside its lock, on the last
release before freeing the slot. The wrapper loads the initialized typed value
and invokes ordinary instantiated drop glue, including `Close` through Option
and generic members. Neither receiver-structural cleanup nor a Meta16 fallback
or cleanup mask is used at this ownership boundary. Copies share the cell, not
the contained `T`; only the final reference owns releasing its value.

Task and channel adapters share the iterator's typed slot descriptors rather
than introducing another erased value ABI. A native SendFn body writes its
result directly into an uninitialized slot; Sema's sendability facts authorize moving
the body and its captures to a scheduler thread. The existing task, task-group
and channel kernels retain responsibility for ordering, cancellation and error
policy. Consuming operands use checked physical ownership facts; independent
owned place reads do not rely on a backend sink heuristic. A task group cannot
consume a borrowed list header, and join/detach cannot consume a borrowed task.
Loaded code and its destructors must
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

The Jet channel state owns a homogeneous payload descriptor and a ring buffer;
ordinary send/receive moves need no per-message owner allocation. Bounded sends
count both queued values and prepared-send reservations against capacity.
Reservations retain physical queue state across preparation, and commit checks
close again before transferring a payload. Single sender/receiver wakeups are
LIFO; close wakes all registrations in insertion order. The selector probes
receiver arms first in source order, then timer arms, and rechecks readiness
after registering its park slot. An `else` probe reports arm -1 without parking.
`jet_rt_native_receiver_iter(receiver, by_value, meta)` copies the supplied
descriptor before its first pull; a receiver iterator can therefore be created
before any sender initializes the channel descriptor. Iterator kind 28 delegates
pull and drop to the channel owner, including closed and stopped pulls.

Shared cells retain their payload and recursive descriptor independently of
endpoint handles. Callback reads can run concurrently; edits are exclusive,
and waiting writers precede new readers. No recursive-lock exception is added.
Callback environments are borrowed, and callback outputs remain unread on a
delivered stop. Every callback releases its permit even when stopped; payload
destructors run outside the permit lock. Process/World site publication returns
the borrowed winning cell, drops losing candidates through the uniform Shared
drop door, and registers only World sites for quiescent reset. Reset preserves
outstanding clones and retains registry nodes for later republishing.
Revision exhaustion is checked before invoking an edit callback and becomes a
task panic inside a task body, not an initialized callback result. The runtime
fixtures exercise prepared-send rollback, stopped iterator/output ownership,
failed-send payload disposal, simultaneous read permits, writer priority and
revision exhaustion. Their Closed crossing uses the actual Rust Prelude enum,
and their paired one-million-message ping-pong records Jet and Rust timings.
Once selection has transferred a payload/arm to the caller, a later stop does
not discard that completed selection. Select deregistration returns a consumed
wake to another receiver when an earlier arm won but this channel still holds
a queued item. Paired private-kernel witnesses exercise the same handoff in
Jet and the Rust seed; no tier-specific wake-stealing policy remains.
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
| `Overflows` | `mov rax, l; add/sub/imul rax, r; seto r8; movzx`; fused into the block's `jcc` when the branch is its only use, and then without the `mov`/`add` when the Binary right before it already computed `l op r` (`add; jo`) |
| `Load` / `Store` | `mov r, [base + off]` / `mov [base + off], r`; 1/2/4-byte layouts use zero-extending byte/word/dword loads and matching stores |
| `DataAddress` | `lea r, [rip + disp32]`, a data relocation |
| `Call` | stack arguments pushed last to first (8 bytes of padding first for an odd count), Float arguments `movq xmm_i, r`, then parallel moves into rdi, rsi, rdx, rcx, r8, r9; `call rel32` with a function or runtime relocation; `add rsp` for the stack arguments; a Bool result is `movzx rax, al` first (System V defines only al), a Float result `movq rax, xmm0` |
| Float `Add Sub Mul Div` | `movq xmm0, l; movq xmm1, r; addsd/subsd/mulsd/divsd xmm0, xmm1; movq dst, xmm0` (Float `Xor` is the bit operation, used for negation) |
| Float `Compare` | `ucomisd` with the operands ordered so `a`/`ae` answer `< <= > >=` (false on NaN); Eq is `sete` and `setnp`, Ne `setne` or `setp`; never fused into the branch |
| `Convert` | `cvtsi2sd` (word to Float) or `cvttsd2si` (Float to word, truncating) through xmm0; integer width changes keep the low bits with `movzx` or a dword move |
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
a frame pointer; its call frame information is in each function's FDE
(Debug information, below).

## Debug information

Native objects carry DWARF 5 and `.eh_frame` (`Image/Dwarf.jet`), so gdb and
lldb break on source lines, step, show source, unwind through Jet frames and
print parameters and locals. Each layer records what only it knows:

- **Lowering** (`Lower/Debug.jet`) fills `LIRModule.debug` and each
  `LIRFunction.debug`. Every LIR instruction and terminator records the
  source line of the MIR instruction it was lowered from
  (`LIRBlock.lines`; the instruction's `source_line`, else its span's line
  in the function's source file; generated code keeps the current line).
  A lowered function names its source file, line and variables: its MIR
  parameters, then every named local with a carried slot. Whenever a slot
  takes a new LIR value (`lower_set_slot`), the value is bound to the slot's
  variable. Generated functions (glue, helpers, the generated entry) have no
  file and no variables. Types are described in their stored form from the
  value layouts: scalars are base types (Char as 4-byte UTF, fixed-width
  integers at their width), structs and tuples are structures of their
  stored members, a List or Map is its `[len, cap, data]` header with
  `data` pointing at elements (or at a slot struct when an element is
  narrower than its stride), a fieldless enum is an enumeration, a payload
  enum or Result is a pointer to its box (the tag as an enumeration, then a
  union of the variant payloads at offset 8), an Option is a pointer to the
  value it holds (null for None), a runtime handle is a pointer to an opaque
  named type, and a closure is an untyped pointer. A variable whose value is
  an inline aggregate is described by the memory its pointer addresses.
- **Instruction selection** returns, per function, a line row where each
  run of code from one line starts (the row at the end of the prologue is
  `prologue_end`), the call frame instructions of the frame above (CFA
  `rsp + 16` after `push rbp`, `rbp + 16` after `mov rbp, rsp`, callee-saved
  register k at `CFA - 8 * (3 + k)`, and `rsp + 8` at each `ret`), and for
  every binding the code range of its value's allocation interval with the
  value's register or rbp-relative slot. Where two values of one variable
  overlap, the later definition wins.
- **The object** gains `.eh_frame` (one CIE, one FDE per function),
  `.debug_abbrev`, `.debug_info` (one compile unit over `.text`, language C11
  for the debugger's expression syntax, one subprogram per function with
  frame base rbp and each variable located by a location list),
  `.debug_line` (one sequence per function) and `.debug_loclists` (offset
  pairs from the start of `.text`). Addresses and section offsets are
  relocations against section symbols, so any ELF linker places them.
  `LIRModuleDebug.directory` is the compilation directory: `jet build`
  sets the source root its source paths are relative to; empty means the
  debugger's working directory.
- **The in-process linker** keeps an FDE exactly when its function is live
  and registers the table at startup, as for the runtime's own. After the
  loaded image it writes the `.debug_*` sections of the directly linked
  objects (relocated; archive members contribute none), a `.symtab` of
  every live function and object symbol, and section headers for the
  image's parts.

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
  ELF64 `ET_REL` with `.text`, `.rodata`, `.rela.text`, `.symtab`, `.strtab`,
  an empty `.note.GNU-stack`, `.eh_frame` and the DWARF sections (Debug
  information, above). A global `main` calls
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
return carrier. `lir_lower_callables(program, roots)` preserves failure carriers
instead of normalizing them as program entries do. All roots share one
monomorphization inventory; `callables` maps original MIR identities to native
entries and `entry` is the first root. Static initialization and the image's
single finalizer are shared across those entries. Compiler-value marshaling borrows
runtime String bytes through `jet_rt_string_data` and
`jet_rt_string_byte_len`, copies before dropping the native owner, and never
depends on Rust's private String representation.
`x64_comptime_drop_scalar` consumes each encoded/returned scalar owner after
detachment, according to the checked callee contract; decoding itself borrows.
`lir_lower_callable` returns an optional zero-argument Unit `finalizer` for
runtime-owned static fixups. Pass it to `x64_call_image(module, entry, finalizer)`.
Its shared initialization flag prevents repeated allocation; finalization
clears that flag and every String/BigInt fixup cell while releasing their
canonical owners. The loader validates the finalizer's code offset before
mapping. `x64_unload_image` runs it before unmapping, only after every native
input/result owner and returning frame has gone. Unload is an exactly-once
operation; this is image-owned static teardown, not a worker sweep of leaked
frame owners. Ordinary owned SSA temporaries still require canonical MIR
retirement edges before a persistent compiler worker can be enabled.
Signed and unsigned 64-bit compiler values preserve their full bit patterns;
unsigned values above `i64::MAX` detach as decimal `BigInt` compiler values.
Exact Int is a trivial lowering word, including promoted nodes: node lifetime
belongs to the canonical runtime's reference/retirement machinery, not a new
boundary drop rail. An Int stored beyond its producing turn uses the published
`jet_rt_int_clone` retain projection. Stringifying it borrows the Int and owns
only the temporary String, which must be dropped after its bytes are detached.
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

The Jet runtime's execution leaves reuse that same request state:
`jet_rt_execution_begin(fuel, depth, comptime)` enters one canonical control,
`end()` flushes the Host-tail's real stdout and stderr streams before restoring
the prior scope, and `stop_write()` reports the current control's record without
consuming it (zero when absent, one when present). The one 48-byte request root
holds mode at offset 40; a successful ordinary comptime begin resets it to true,
and an unsuccessful execution begin leaves the active mode untouched. Budget
sites are present only in comptime mode: `U64.MAX` remains a present value, not
an absence sentinel. Both writers share one Stop56 JSON/framing encoder,
preserving ordinary comptime's prior bytes. Encoding snapshots the borrowed
record under its owner's mutex; channel writes happen after unlocking.

`Tests/execution-fixture.c` compares both budget writers byte-for-byte and checks
the historical Scheduler encoder goldens for cancellation, deadline and a real
Task-body panic, escaped UTF-8, failed-begin mode retention, full-range sites,
nonconsuming reads, empty records and partial stdout flushing. Its native proof
requires a freshly composed UNIT1 **and Host-tail** pack; source parsing and
C syntax checks alone are not runtime evidence. The runner accepts that linked
fixture executable: `node Compiler/JetRuntime/Tests/run-execution-fixtures.mjs BIN`.


## In-process linking

`Image/StaticLink.jet` (`x64_link_executable(names, bufs, base)`, #4549) links
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
- **Layout.** ET_EXEC at the link base (`LINK_BASE` 0x400000 for `jet
  build`; `LINK_RUN_BASE` 0x200000000000 for an image `jet run` maps into its
  own process, `Image/Loader.jet`) with file offset = address - base, three
  page-aligned `PT_LOAD` segments: read-only (headers, `.rodata*`,
  `.gcc_except_table*`, the kept unwind records plus a zero terminator),
  read+execute (`.text*`, padded with `int3`), read+write (`.tdata`, the
  init/fini arrays, `.data.rel.ro*`, the GOT, `.data*`, then `.bss*` and
  common symbols), plus `PT_TLS` (`.tdata` + `.tbss`) and `PT_GNU_STACK`.
  Section headers, `.symtab` and debug sections follow the loaded image
  (Debug information, above).
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

`Image/RuntimePack.jet` (`x64_link_runtime_pack(names, bufs, extra_roots)`)
packages the checked Jet runtime object and its permitted C platform archives
as one self-contained ELF relocatable object. A program link reads its own
objects plus that pack; no Rust runtime object or standard-library archive
participates.

`Tools/packaging/prebuild-runtime.sh` delegates to `prebuild-runtime.mjs`.
The producer assembles `Compiler/JetRuntime/sources.list`, invokes the Jet CLI's
library-object hook, and links its object with static musl `libc.a` and the
selected C toolchain's `libgcc_eh.a`/`libgcc.a`. The hook explicitly disables
Rust emission and uses `x64_runtime_library` followed by the in-process pack
linker; it never invokes rustc or a system linker. Missing Jet exports or
unresolved platform imports are failures, not a Rust fallback.

The published `jet-runtime/<key>/jet_runtime.pack.o` has matching ABI, export
and provenance records. The key covers checked runtime source, ABI, backend,
lifecycle and C platform identities; `native.key` selects it for the CLI.
The producer and linker reject Rust-mangled/allocator symbols. Historical
`prepare-call-runtime.mjs` seed-Rust adapter assembly is a separate bootstrap
test path and is not the native pack producer.

- Resolution pulls archive members for the checked Jet C exports, platform
  startup and explicit extra roots such as `memcpy`. COMDATs and weak/common
  symbols are settled, and the startup object is included. Rust allocator
  shims are not admissible pack dependencies. Only the roots stay global;
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
were compared. The historical mixed-runtime sample had a 3.0 MB pack
(8998 section headers, 66 exports); hello-world kept 3136 of its sections.

Historical mixed-runtime link time, measured with the linker compiled by
`jet build --release` (the Rust-emitting tier, not Jet-backend output), inputs
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
- `x64_unload_image(loaded) -> Bool` runs its optional checked finalizer, then unmaps the block; no native owners or returning frames may remain.

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
shared task-control stop record distinguishes bounded comptime fuel/depth and
cancel/deadline stops from fatal panic/trap. A turn's `#MaxDepth`, stack or
memory exhaustion also invalidates its session (D-REPL-FAIL1=A and
D-RESOURCE-LIMIT1=A); bounded comptime depth is not a turn's resource limit.
No `longjmp` may skip owners' drops. Fatal stops terminate the child and
invalidate every live binding while preserving output already emitted.

The Jet service (`Image/SessionHost.jet`) owns the runtime and all mapped
images on one thread. The CLI private `--native-session-host` entry starts a
fresh process with `posix_spawn`, not a fork of compiler locks. Its fd 3 is
one private duplex socket: decimal byte length, newline, exactly that many
body bytes. JSON metadata/value frames retain the canonical parser's 16 MiB
limit. Native images instead use raw binary chunks (at most 64 KiB each),
with an exact total length and terminating SHA-256 digest; bytes are never
hex-expanded or interpreted as UTF-8/JSON. The receiver rejects oversized,
truncated, or mismatched image streams with a host-error response, not a
partially admitted image. User stdout/stderr are separate byte streams and
never participate in framing. Descriptors share one process-owned, serialized
child, which starts lazily and is explicitly closed at the compiler host's exit.
`JetDriverCompileRequest.execution_host` is the compiler's host capability.
Both comptime and turn requests use this child and canonical task control;
each request ends its scope before the next starts. Successful scalar replies
tag their type and carry exact integers/IEEE bits as decimal strings.
Compiler-generated wrappers return type-directed JSON, never Display
reinjection. The child verifies the admitted runtime SHA-256 before Ready;
each image request names that same artifact. Packaging emits the native rlib
and resident `.so` in one rustc invocation, links the pack from that rlib, and
records both artifact digests, producer digest, tree identity and actual
execution/slot exports in `MANIFEST.json`.
Stopped calls never read or drop their uninitialized output cell. Fatal
records retain exact messages and optional sites; a signal without a record
becomes Trap with absent site fields. The host reaps and starts a fresh child,
but never retries a failed request, reinstalls previous images, or replays
side effects.

The first frame distinguishes lifecycle EOF from malformed framing: EOF before
any prefix byte and explicit `Shutdown` close normally; partial prefixes,
invalid digits, oversized/truncated bodies, and non-UTF-8 metadata return
`HostError`, which the compiler maps through registered E0956. They are not
language stops and admit no partial result.

Typed results use counted preorder `Node` metadata, not one nested JSON item.
Each node names its kind, checked field/variant/capture attributes, child count,
and raw text byte count. String and exact-Int text travels in borrowed chunks of
at most 64 KiB; the receiver parses each node header once and reconstructs the
compiler value without joining a JSON body. `ValueEnd` admits the complete
result only after the native scope has ended. The selected value callable writes
canonical result storage; only a successful return may stream or invoke the
image's original typed release glue. The allocation and free use identical
size/alignment, including stopped-but-uninitialized storage.

Logical `Close` destroys that scope's typed slots before retiring its closure
registry rows and unloading its dynamic images in reverse definition order.
Repeated close is idempotent. Reuse of a closed logical identity is a nonfatal
host refusal: it does not restart the child or discard another scope's owners.
Retirement metadata lives in the compiler-owned host descriptor, shared by
descriptor copies, and survives a child restart; a fresh generation cannot
silently recreate an explicitly closed logical scope.

Shared-site placement requires a distinct process-pinned Core image. Every
Process/World/thread-key site lives once per initializer identity in that image;
dynamic images import the pinned getters and never emit private site cells.
The pinned image is loaded once per child generation and is excluded from
logical-close and dynamic-image unload. This producer/linking prerequisite is
not replaced by an unload-time `global_forget_range` export.

The private `Core` lifecycle frame supplies the image's bounded metadata and
raw chunks before an ordinary call. `CoreReady` acknowledges admission; the
compiler descriptor remembers its identity and child generation so subsequent
turns do not resend it. Identity includes both code bytes and layout/export
metadata. A different identity in the same child is a nonfatal host refusal,
not a replacement or an implicit reset of Process sites.
Before publishing exports, the child runs the producer's checked no-argument
Unit Core startup under the canonical execution control. It initializes only
frozen static fixups, not user entry code or Shared payloads. A stop prevents
publication and retires that child; reusing an admitted identity never reruns
startup.

`X64MemoryImage.exports` maps `jet_core_shared_{initializer:016x}` to actual
emitted function offsets. Names must use sixteen lowercase hexadecimal
digits; offsets must name functions inside executable code. Duplicate names
or colliding export offsets are rejected before any address is published.
Shared getter imports resolve only through this pinned inventory, never
through a runtime-library fallback. Closing a logical session cannot finalize
or unmap the Core image; a restarted child admits its own fresh image.

Source runtime evaluation delegates `GlobalShared` to the typed host
`SharedSite` target. Each factory keeps only its local logical registry; the
persistent parent host owns the canonical pinned Core image and its physical
Shared roots. No old factory's typed evaluator context, Rust type identity,
adapter or callback projector may be retained across factory unload.
The parent and native child are separate physical namespaces: no live payload
or pointer is serialized between them. Attachment compares the canonical Core
owner identity and seal plus the sorted site/lifetime/payload-layout/getter
inventory before invoking a getter. A mismatch is a registered named E0956
refusal: old Process state stays intact and incompatible new code cannot attach.
World resets at the mapped one-shot/reload boundary; Thread persists until OS
thread retirement, and Attempt resets only at the selected-unit attempt boundary.
The Driver's `CoreLibrary` graph roots the complete installed catalog, including
private parts, independently of any user import closure. Its registration anchor
is the first canonical module-owner/path row, not a synthetic `run`; every
checked Core body passes through the ordinary TIR/MIR pipeline. The retained
recipe is normalized by `jet_driver_core_library_sources`, and
`jet_driver_prepare_core_library` computes its expected manifest before the
native producer can lower an image. Factory accessors use the same checked
Source callables to recompute that schema from the retained recipe.
The parent runtime admission record is private, build-time embedded metadata
from the same producer as the host binary and runtime library, not a user flag,
environment lookup or receipt self-hash. `JetDriverCoreSourceParentHost` retains
the canonical recipe and parent without a factory context. Jet's
`CoreParentSeed.jet`/`CoreParentIngress.jet` generator binds the seed transport
from checked emit metadata; the seed has one static resident host and invokes
the fresh factory's Source preparation before admission or getters.
Factory provenance is the canonical private Entry path or the aggregate
assembler's exact first-line private-host header; comments elsewhere never
grant ingress. The header shifts generated offsets only, with all original
source-map byte ranges and diagnostic line/column locations preserved.
The Rust splice accepts only the generator's build-time definition marker;
a runtime-producing build without it refuses with `CoreSharedSeedIngressMissing`.
Stage zero without a runtime retains its existing bootstrap path. Native
physical Shared interop and composed runtime execution still require integration.

The boundary's ownership rails are fixed:

| Carrier | Retained owner | Release rail |
| --- | --- | --- |
| Native closure box | Native refs at 0; emitted code at 8, emitted release at 16; captures from 24 | Clone increments refs; last unref invokes emitted release, drops by-value captures and frees the box |
| Source callback | Originating `JetEvalMachine.callback_roots`; retain copies reusable roots, transfer consumes one-shot roots | Checked `jet_eval_callback_release` removes the root exactly once; Source drop frees captures |
| Native callback envelope | Source callback/job session registration; job owner remains live through cleanup | Owner pump handles Release via checked Source release followed by release completion |
| Canonical Core parent | Original source recipe, admitted runtime, pinned native image and digest-only attachment | Never retains a factory evaluator, callback root, projector or typed context |

Closure code and release resolve only through actual emitted callable rows.
Missing exports are named refusals; the generic concrete-layout/provenance gap
remains a named refusal, not an ID/offset guess or a copied factory root.
Payload slot clone/drop lookup additionally checks the exact semantic identity,
role and type key, a SystemV void signature, and two/one Pointer8 parameters.
It returns only the actual parent-pinned address; missing or mismatched typed
roles refuse before touching a slot. The Driver accesses the same admitted
canonical inventory through its digest-checked parent attachment.
Generic and payload-role callers share one admission/inventory/image lookup.
The factory fetches the immutable canonical manifest once per admitted
attachment and reuses it for its per-type payload codecs, not once per payload.
`Driver/SharedPayloadCodec.jet` stages checked data payloads recursively over
that inventory. Its per-call journal owns allocations and initialized typed
slots: failed children unwind in reverse order, and a completed parent takes
its children into one actual typed drop action. Missing child drop bindings
refuse before materialization; no guessed leaf helper repairs that provenance.
Lists (including byte lists) use the admitted element stride, Maps use the
entry row, and boxed members use their storage-wrapper box size. Binary32
conversion preserves the exact widened IEEE word instead of rounding it.
Result selectors and payload offsets come from the ordinary Result definition
in `Lower/Values.jet`, shared with `lower_result_box`, not a codec-local ABI.
Map ordering propagates missing layouts and mismatched key shapes as named
refusals through a fallible stable index merge before native materialization.
Replacing a payload moves the old owner into a returned retirement journal
under the original held physical guard; the caller publishes/releases that
guard before invoking old drop glue, so reentrant cleanup cannot deadlock it.
This data codec alone does not supply a factory's physical native-slot
capability/projector or turn a Source callback root into a native closure.

The canonical ordinary-layout inventory records `Float32Bits` explicitly.
A Map's third child is its ordinary entry-tuple layout; that row's stride,
not the Map header's stride or a member-offset calculation, spaces entries.
Each member records both its logical type key and ordinary storage type key.
A boxed member uses the latter's canonical Option box size, which differs
from packed element stride (for example U8 stride 1 versus Option<U8> box 8).
The same row seals its ordinary `mir_type_display_name` once for naming absent
payloads; codecs must not reconstruct that name from erased List shapes or
physical fingerprints. This name carries no physical layout facts.
These facts are sealed in manifest body version 2. They do not change the
runtime's 72-byte NativeMeta layout or its two child pointers at 56 and 64.

Native payload slot access uses the promoted readers and checked writes in
`OS/Linux.jet`. Only widths 1, 2, 4 and 8 are admitted; invalid addresses,
widths and write ranges refuse before memory access. Allocation receipts retain
the original free address and exact positive size/power-of-two alignment;
release refuses a mismatched layout rather than truncating or guessing it.
`core_shared_runtime_call` consumes the admitted runtime's sealed `export_abis`
and symbol addresses. Its fixed zero-to-three-word argument path allocates no
argument list, rejects mismatched arity and Float/Memory/wide carriers before
invoking code. Parameters must be eight-byte words: narrower parameters refuse
because the sealed layout does not encode signedness. The only admitted narrow
result is Integer/1, with a Bool/U8 zero-extension contract (reached by
`jet_rt_native_shared_weak_upgrade`); size-2/4 results refuse rather than silently
zero-extending signed values. Refusals identify the export and unsupported
class/width. These helpers do not imply runtime or Core admission.

Native stored-owner destruction uses the checked MIR `Close` implementation,
when present, with its concrete receiver bindings and consuming call convention.
That call owns cleanup; the slot destructor does not also drop its members.
Without `Close`, the existing structural destructor applies. Only the original
consuming receiver retains a structural cleanup role; other same-typed values
and a rebound receiver retain ordinary Close. Whole transfer invalidates that
original receiver identity. A fieldless resource is still nontrivial. The Shared
constructor receives the exact `NativeMeta.drop` callable as its third argument,
so last-owner retirement uses the same destructor as other owned native slots.


`x64_memory_core_shared_image` consumes the explicit pinned-Core lowering
inventory, rather than guessing exports from MIR function numbers. Getter names
are `jet_core_shared_` followed by the initializer's sixteen lowercase hex
digits. Each export offset comes from the linked compiled function identity,
must lie in executable code, and must have the no-argument System V
eight-byte Integer/Pointer result ABI. Each SiteKey must be exactly
`shared-site:{initializer.value}` using the initializer's decimal identity,
the same rule used by the producer and legality verifier. Noncanonical
SiteKeys or names, duplicate symbols, missing functions, and invalid signatures
fail image construction. A site-free pinned image is valid and has an empty
export table, as do ordinary dynamic images.
Checked closure source is retained separately from MIR. Codegen records each
original `TLambda` with its `MIRFunctionID` and source file, carries nested
receipts through ordinary and incremental lowering, and the compiler host
indexes them by `(mir_program_digest(snapshot), MIRFunctionID)`. That digest
includes the checked program and its original source files. Looking up only a
numeric function ID in the current compilation is incorrect: older pinned
images may reuse it. Keeping this table does not by itself admit native closure
detachment; the typed value boundary must also supply capture layouts and an
ownership/drop receipt before portable closure values can cross it.

#### Native echo and live references

An echo retains its checked static type and exact `Show` text independently
of its value transport. Pure data additionally carries the type-directed,
lossless JSON selected by `eval --json` option B: exact integers and floating
bits are not coerced through a JSON floating-point number, and structural
variants, fields and map keys retain their checked identities.

A non-portable value instead carries a child-owned typed reference:
`{generation, binding_or_value_id, binding_name, static_type}`. Its payload,
closure captures and executable pointers remain exclusively in the child.
The reference is not a rendered string to parse back into a value. Its ID is
stable within the owning session; shadowing and later values receive new IDs.
Its checked type comes from the compiler's semantic facts, never from `Show`.
The reference's owner controls its lifetime, and its defining image/drop glue
stays pinned until that owner is retired.

After a fatal restart, a reference with a different generation is refused
with a registered diagnostic that names the binding and says it was lost
when the session restarted. No stale pointer, scalar-only fallback, replay,
or implicit clone transaction may manufacture a replacement value.



## Stop points

The unified execution host (#4720) runs session turns and native comptime in
one persistent child. Interrupts, deadlines and comptime fuel/depth limits
stop recoverably inside that child and leave through explicit MIR cleanup;
a panic or trap invalidates the child and the host restarts it. MIR carries
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

Only host compilations produce stop points (`JetCodegenProgram.stop_points`;
for comptime the driver's `JetDriverCompileRequest.comptime_stop_points`,
set by the native comptime selection); the Rust and web emitters, the
evaluator and the inliner refuse them. O0 lowers `Check` to
`jet_rt_session_poll()`, `Propagate` to `jet_rt_session_propagate()` (any
nonzero word takes the stop edge) and `Stopped` to a return.

Comptime budgets are can-stop operations too (`comptime_budget`, compiler
callables only): a Prelude call `jet_rt_comptime_enter(start, end)` after the
parameters and `jet_rt_comptime_step(start, end)` at each loop continue
target, each followed by its Propagate, and `jet_rt_comptime_leave()` (depth
bookkeeping, no poll) before each Return. MIR carries the site's source span;
O0 prepends the function's owner row in the program it lowers, the dense row
the stop record names. When fuel or depth runs out the runtime publishes a
BudgetFuel/BudgetDepth stop on the task control and returns; the Propagate
takes the stop edge.
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

**Codec reachability.** Derived serde impls are emitted only for a checked
use of their codec, per codec: a call of one of the impl's methods, a
codec-bounded runtime route (the `CODEC_ROUTE_PREFIXES`/`_LEAVES` symbol
families: typed decode, `json.to_string`, HTTP JSON, jobs, sync, data, App
handlers) at the types in its type arguments, operands or result, a user call
whose callee's generic parameters carry the codec bound, and the signatures
of exports and jobs, whose wrappers decode inputs and encode results. A
demanded impl's methods are scanned in turn, so a derived body demands its
field types (`Compiler/JetFoundation/Source/MIR/Reachability.jet`
`mir_demanded_codec_impls`, MIRRust `demanded_codec_impls`; both emitters, every
program). The drift test `codec_bounded_runtime_functions_are_codec_routes`
checks every codec-bounded Prelude function against the route families. A
missed demand fails rustc with E0277 rather than changing behavior.

**Proof.** The repro above and both `Core/app/app.jet` patterns, compiled
from the Rust compiler's checked MIR through the Jet emitter (MIR from
`JET_DUMP_MIR`, converted by `Compiler/JetBackend/Tests/mir-debug.mjs`), give
bodies that rustc accepts; the same bodies in dispatch form fail with E0502.

## Testing

The stage0-built golden driver keeps trimmed asm operands as borrowed views:
matching a named register uses a guarded prefix removal and requires the
remaining suffix to be the closing brace, rather than calling owned-String
`slice` on a trimmed view. The `clobbers ` comment prefix uses the same guarded
removal. Binary-pattern bit widths remain checked `U8` and are explicitly
widened with `Int.from_u8` at the Lower constant boundary; stage0 does not
construct an Int directly from a width's fixed storage.

The golden-lowering driver's generated package grants `FS.Read`, `FS.Write`,
`IO`, `Mem.Alloc`, `Panic`, and `Time`: these are the checked Lower unit's
required effects. The harness must grant `Panic` for its checked-invariant
failures as well as its file, allocation, and timing operations; otherwise
driver compilation refuses before any golden can be lowered.

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

The native ownership rail additionally writes `record-int-ownership.o`,
containing a `BigIntHolder` whose value is `4611686018427387905` (beyond the
inline Int range). With `JET_RUNTIME_C_LIB`, the checker links that object
against the real runtime and `Tests/check-record-int-ownership.c`. Linker
wrappers delegate to the real allocator and Int retain/drop exports, checking
two retains, three drops, unchanged getter bits, and zero live allocations
after both record owners and the owned getter result have been dropped.
Runtime domain initialization is warmed before the counted interval. This
rail exercises stored-field ownership only: primitive Int SSA ownership is
unchanged. Inline fields skip the runtime retain/drop call entirely.

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
instead of `<case>.o`; an uninstrumented object is now a coverage failure.
Native frames carry no redzones, so stack slots are not checked. A run without
a report must also reproduce the plain native stdout, stderr and status.
Empty corpora, missing requested cases/references, link failures and crashes
cannot pass. `--runtime-library ARCHIVE` uses a prebuilt instrumented native
runtime without invoking rustc; `--no-leaks` drops LSan and records that choice.
The standing O0/O1/O2 gate and producer-admission contract are described in
[`safety.md`](../spec/safety.md#standing-native-safety-gate); absent tier/runtime
coverage is explicitly non-green.

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
native divergence/absence and any lint finding/absence. Sanitizer coverage,
when requested, must be complete. A case reproduces from its seed.

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

For compiler-package inventories, `Tests/run-self-front.mjs OUT --front jet
--unit SELFHOST.jet --map compiler.map.json --candidate CANDIDATE/jetc0`
selects the observe-only `JET_FRONT_MIR_OUTPUT` entry. The bootstrap aggregate
includes a schema-generated Jet writer; the backend's native path still consumes
live MIR, never that observation stream. `self.mird` is one entry ID followed by
the checked program in schema order. `self.functions.tsv`, `self.sources.tsv`,
`self.inventory.json` and package slice ID tables describe that same program;
they do not admit native execution or independently compiled package programs.
Original-source segments map aggregate spans to compiler packages; generated
writer spans are separate `mir_observer` provenance, and unmapped functions are
retained rather than silently omitted. Decoded enum discriminators use
`$variant`, preserving checked payload fields named `variant`. The codec checks
every current operation variant independently of the observed compiler corpus.

The native unit-test producer is `lir_lower_tests(program, roots, checks, serial)`.
It shares one monomorphization inventory across selected checked tests and output
checks, and emits an AOT entry plus task callbacks. Each test callback runs the canonical
test rail through `CAbiTest`; panic classification, expected failures, skip,
captured print, and the final report are not reimplemented by the parent.
Property generation/shrinking is explicitly refused until its Jet implementation
exists.

`Cli/NativeTests.jet` owns selection in canonical module/source/name order,
filtering, the identical SplitMix64 shuffle, and the native child runner. A
child appends lossless, byte-counted records to the process-private file named
by `JET_NATIVE_TEST_RECORDS_PATH`, never stdout. `JET_NATIVE_TEST_ID` selects
one original checked MIR function; absent that selector the child spawns all
selected test tasks before joining, or joins each immediately in serial mode.
After a fatal child, only tests lacking complete records are run individually,
in parallel unless serial was requested. A non-reproducing fatal batch remains
an error, never a fabricated successful run or an attribution to another test.
The parent preserves ordinary child stdout/stderr separately, orders canonical
outcomes by the selected identities, and invokes the same executable with
`JET_NATIVE_TEST_REPORT_PATH` for the canonical finish and exit status. Protocol
I/O failure exits 103 and is an infrastructure error, not a failed test.
`Host.jet` dispatches non-release tests through the NativeTests module API;
release tests retain the existing emitted harness path. Output checks run once
in the same successful batch process after body joins, with static initialization
and checked result ownership; Bool checks assert, other results are discarded
as in the canonical runner. After a fatal batch each unrun output check is an
explicit failed canonical outcome carrying E3015 (what/why/fix); the fresh
reporter never executes checks against recreated process state. Custom TestSuite
overrides remain explicitly refused on the native path. Full native/CLI proof
must precede admission.

Build the candidate and runtime pack from the same composed tree. Matching
goldens do not prove full frontend equivalence; compare the two modes on the
same complete case list, retaining both `results.tsv` files and every
failure, before admitting parity.

The consolidated proof consumers are `Tests/check-front-feed-results.mjs`
(exact full398 selection, no lost native passes, zero wrong outcomes; reports
both pass counts, every differing verdict/detail and ranked diagnostic roots),
`Tests/test-native-test-host.mjs` (three Examples and canonical capture,
filter, serial and shuffle policies), and `Tests/test-native-test-contract.mjs`
(output checks observe joined batch state; a fatal child preserves completed
outcomes and reruns only unreported bodies, never output checks). The registered
E3015 what/why/fix snapshot is exercised by
`Tests/test-native-output-diagnostic.mjs`. These consumers require the real
composed candidate, host and matching runtime pack; metadata-only checks do
not admit execution parity.
Its registered What line starts `Output check not run:` in sentence case,
matching the canonical snapshot and the shared user-facing case-law gate.


`Tests/run-front-feed.mjs` selects **every** `pass` row in an explicit native
`results.tsv` baseline. It assembles the compiler source inventory,
`Tests/FrontFeed.jet`, and schema-generated observation codecs into one probe.
The Jet feed passes the driver's live MIR directly to the backend; only the
independent Rust feed uses Debug conversion. Both feeds link the same runtime
pack and run in separate, identically staged working directories with the
golden's stdin. The gate requires matching stdout, stderr, and status between
feeds, and successful stdout agreement with the golden for each feed.
The Debug comparator maps Rust `PtrFromAddr.addr` to checked Jet
`PtrFromAddr.address`; it still rejects an absent address instead of inventing
one. The Jet observer writes the checked field directly.

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
Field declaration defaults are excluded from the parsed type: `String?{None}`
is an optional String, and a defaulted `[T]{...}` is a List of T. Defaults do
not supply missing checked Rust fields; a changed schema still requires freshly
converted checked MIR rather than reusing an older decoder's token stream.

Full MIR observations are retained per case. `gaps.json` and `gaps.tsv` rank
all observed schema, type, layout/ABI, ownership/drop, route, and generic
differences by the number of distinct affected goldens. IDs scoped to source
authority are compared through symbolic names where pairing is unambiguous;
ambiguous pairing is reported, never treated as equality. `--bootstrap JETC0`
provides a front-end diagnostic census before the probe can be built.
Preparation and a receipt-only census never establish differential parity;
only `summary.json` with `differential_verified: true` does.

### Native arena resource counters

`MemRegistry.jet` is the single native owner of the seven memory observation
words from `Prelude/Observe.rs`: outstanding arenas, allocations and bytes,
retained block capacity, high-water bytes, and the two allocation-probe totals.
Its priority-1 runtime initializer allocates its own state after the heap is
ready; it does not share allocator policy cells or count ordinary heap blocks
as arenas. The provider uses the existing `Sync.jet` atomic leaves.

Core's Arena/Bump/Fixed/Pool implementations call the open/alloc/reset/close
hooks, and their RawBlock owners call retain/release. Zero-sized values count
as allocations with zero bytes. Counters carry unsigned machine-word bits,
wrap on fetch-add/subtract, and snapshot independently with relaxed loads,
matching the reference rather than promising a transactional metric snapshot.
The high-water candidate alone saturates on unsigned overflow. Probe reset
and take use sequentially consistent exchanges and never clear live facts.

`jet_rt_mem_resources(out)` writes three words (arenas, allocations, bytes)
into caller-owned 24-byte aligned storage for the Observe live writer.
`MemRegistryWitness.c` links the real native provider and covers lifecycle,
zero-size, unsigned wrapping, probe isolation and concurrent allocation totals.
Compile it with `cc -pthread` against the composed runtime object; its success
line is `MemRegistry: lifecycle, zero-size, probe isolation, unsigned wrap,
concurrent totals ok`. Source/syntax checks do not establish native parity.

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

For the combined Text/collection exit proof, both runners first write their
`--plan` selections. Run the union once, then pass that fresh `results.tsv` to
each runner with `--results FILE`. This still requires every selected baseline
and focused stem to pass and runs each layer's raw ownership, stopped-result,
and symbol witnesses; the receipts record the shared result file.

Stopped splits discard both partial lists; owned search needles, zip inputs,
chunks, progress outputs, and unboxed failure payloads are also released before
returning their neutral stopped result. Zip checks the stop rail before padding
or reporting unequal lengths. A partially consumed step-by hint counts only
remaining complete steps, and Float averaging preserves the reference sum's
negative-zero identity.
The focused collection review fixture checks allocator-class balance for split,
owned-needle, chunks, both stopped zip inputs (strict and padded), all three
try-error drains, and stopped progress output. Test-only wrappers delegate the
real free or progress advance exactly once before triggering the real comptime
budget rail; they are absent from the production runtime. Raw witnesses check
fresh and partially consumed step-by hints and the Float average sign bit.
The shared native goldens require strict-zip task cancellation and the exact
`-0.0` average output, not merely numeric equality with zero.
Bits extent at the maximum I64 index stops on overflow: the checked seed
profile explicitly retains overflow checks (`Compiler/Bootstrap/Tests.rs:68-83`);
the unchecked `opt-level=1` pack build (`Tools/packaging/prebuild-runtime.sh:70-71`)
is not the semantic oracle for the plain `v + 1` in `JetBitSet::len`.

The generic Rust symbol census also includes List operations already emitted
directly as Jet-authored LIR, rather than imported runtime functions.
`Compiler/JetRuntime/Tests/lists-native.tsv` records each audited symbol's
dispatcher, concrete body, Rust reference, ownership behavior and supported
type limits. The first batch reuses the existing `Lower/Lists.jet` implementation
(all-ref search finds its introduction in `1e7f948f3`), not a second runtime
kernel. `check-lists-native.mjs` checks source/dispatch/helper consistency;
its receipt is not native execution or unrestricted type-coverage proof.
The second batch records 22 more List bodies in `Combinators.jet`,
`Parallel.jet` and `Lists.jet`, including eager callback ordering, first-error
cleanup and explicit unsupported callback/type shapes. The joined
`jet_rt_native_para_walk` provider is Jet-authored in runtime `Parallel.jet`.
It uses the existing Thread and Scheduler control primitives: fixed 64-item
chunks are assigned cyclically, all workers join before any failure is
published, and the lowest failing chunk start selects the original stop record
(`CAbiParallel.rs:9-33`, `Core.rs:360-420`, `ParallelKernel.rs:60-90`).
Zero worker limit means one worker; negative or outside-I64 exact limits
mean unrestricted capacity. The shared affinity probe uses fallback one,
without the scheduler pool's environment override. `ParallelTests.jet` and
`run-parallel-fixtures.mjs` exercise empty/serial/tail walks, joined multiple
failures, stop-form preservation and allocator-class balance when run against
the freshly composed runtime; source parsing alone is not behavioral proof.
The last census List route, `jet_list_para_fold_add_fixed_f64`, is implemented
in `IterNumeric.jet` with the existing `(Float slots, length, seed)` C ABI.
It preserves 64-element indexed chunks, the seed in lane zero of every chunk,
the eight-lane finish tree and adjacent-pair merges with zero seed/odd carry
(`ParallelKernel.rs:95-124`, `SimdLanes.rs:683-717,785-794`), without callbacks
or scheduling-dependent results. Single chunks need no scratch allocation;
larger inputs use one compacted partial buffer. Raw witnesses distinguish
per-chunk seeding, tree versus serial rounding, odd-partial association and
empty negative-zero seed, and require allocator-class balance.

The legacy nine heap/bisection routes now use Jet implementations in
`Collections.jet`, preserving their existing native signed-I64 input slots,
exact-Int scalar index results and transferred output buffers
(`CAbiRoutes.rs:14034-14239`, `MathLibPure.rs:1287-1438`). Binary bounds keep
duplicate left/right placement; heap operations preserve strict child ties;
sorted merge takes the left equal item first. Top-N selection uses only an
N-element heap and sorts its selected output, avoiding a full input copy for
small N. The raw witnesses cover duplicates, empty input, immutable borrowed
inputs, growth, signed extrema, ordering and per-class allocator balance.

The eight OrderedMap routes and their clone/drop/three rendering rails now
own paired native List headers containing independent E4 strings. Updates
preserve insertion order and return fresh owners; lookup and key/value lists
clone borrowed strings. The raw fixture checks replacement position, missing
lookup/removal, independent clone, NUL text surviving all input/container drops,
exact bytes for all three rendering modes and allocator-class balance.
The retirement predicate fails for these rows: the compiler still pins
Counter/OrderedMap/Layer/Chain to native carriers (`Emit/Types.jet:2637-2640`),
so the Jet runtime replaces the existing ABI rather than deleting live routes.

Counter's fourteen routes and five owning/rendering rails use the same paired
header with native signed-I64 counts. Zero deltas do not introduce keys; zero
totals remove entries; negative counts survive but contribute no elements.
Most-common keeps insertion order for ties and negative K means all entries.
Merge/subtract update one fresh owner with reserved capacity, preserving the
reference's checked arithmetic and ordering without repeated full copies.
Decimal rendering handles I64::MIN through unsigned magnitude. Raw witnesses
exercise all routes, three exact render strings, wide boxed return ownership,
independent clones and per-class balance. Ambient dispatcher rows remain
(`Core.jet:504-517`); these are live ABI replacements, not row retirements.

Chain's four routes and the Chain/Layer clone/drop/rendering rails own a native
list of inline paired Layer headers. Push deep-copies the new front layer and
all old layers once; get returns an independent string from the first matching
key in the first matching layer. Contains performs the same checked borrowed
lookup without allocating a discarded get result. Chain rendering writes
layers directly into one builder rather than allocating intermediate strings.
Raw witnesses cover duplicate keys, front precedence, empty layers, unequal
input lengths, NUL keys/values, independent clone/output ownership, three exact
rendering modes and allocator-class balance. Separate get/contains witnesses
require the checked bounds stop for a matched key beyond the value array.

The native ABI follows the Rust provider (`Core/Collections.rs:3524-3555`):
push retains both complete input arrays, and a matched unpaired key panics.
This differs from the source Core `layer_from` normalization
(`Core/collections/collections.jet:593-605`) which truncates to the smaller
length. The native provider preserves the existing checked ABI rather than
changing Core emission policy or silently normalizing its input.

The remaining 21-row collection census is tracked per symbol in
`JetRuntime/Tests/collections-residue.tsv`. Thirteen paths already lower
directly: three typed LoopIter operations, Option.map, three slices, two view
combinators and four view constructors. Slice/view bounds use the Jet-owned
`jet_rt_slice_end`/`jet_rt_view_end`; the original generic providers are not
called by these paths. Two LoopIter adapter metadata rows still exist, so this
is path provenance, not proof that every generated adapter has been retired.

Six formerly live providers now have Jet exports: the RangeCursor destructor,
four numeric LoopRange operations and inline range_contains. The LoopRange
boundary retains borrowed exact-Int inputs and fresh exact-Int value results;
the private cursor keeps native current/end/step plus exclusive/done words.
Both stride directions and inclusive extrema are preserved. Checked step
overflow ends iteration without replacing current or raising an overflow stop.
The Rust seed's five handwritten cursor C exports are excluded from native
packs in the same ownership cutover as their Jet definitions.

Named Range.contains and Regex.count remain live generated adapters until a
canonical native handle input contract is available. The true Regex count
kernel is `CoreLib/JetStd/Regex.rs:486`, not the older inventory's accidental
match of Bits::count at `Core/Collections.rs:147`. No Rust Arc<RegexProgram>
layout or guessed source Range offsets are interpreted by a Jet provider.
The direct Range-value iterator in `Lower/Iter.jet` is a different path from
legacy LoopRange instructions in `Lower/Lower.jet:1750-1771`. Zero literal calls
alone cannot prove closure: dynamic symbols and Runtime drop descriptors are
tracked explicitly by the consistency checker.

Lazy cursor initialization and advancement may invoke user callbacks. Both
are can-stop MIR operations: their results are usable only on the immediately
following Propagate poll's resume edge. Prefetched HasNext/Value operations
remain non-stopping. A cancelled map callback must report task cancellation,
never return a short successful collection or silently end a for-loop.

File and stdin line producers keep the three-argument native ABI
`(source, by_value, element_meta) -> NativeIter`. The source argument is a
borrowed four-word Jet callback adapter `[payload, next, release, clone]`,
not a Rust owner or an OS descriptor. Lowering supplies typed source-method
callbacks, preserving source buffering, UTF-8 validation, CR/LF handling,
fault checkpoints and `<core.prelude>:0` error rendering. The runtime copies
callbacks into a kind-29 iterator's existing closure fields; no second
producer/context allocation is required. A pull returns EOF (0), a moved slot
(1), or a stopped rail (-1, with an unwritten output). Borrowed file/stdin
sources are eagerly snapshotted; owned sources are lazy. Process producers
retain the shared pipe through its clone callback, independently of the child.
Encoding producers own a checked reader and preserve its nested EOF result.
### Jet-authored runtime object

`Compiler/JetRuntime` is a separate package, assembled from its own
`sources.list`; it is never part of the compiler bootstrap inventory.
`JetBackend/Tests/run-runtime.mjs` sends that checked library directly through
the Jet frontend's library-object hook. The object exports exactly the
checked `#Export(c)` functions, each mapped per position to one C primitive
(I64/U64, U8, Bool, Float/F64 on the System V xmm ABI); `lifecycle.list`
only orders their init/fini hooks.
The runtime's private allocator root uses the shared MIR `.Global` lowering.
`jet_rt_alloc_init` is a void/no-argument constructor in `.init_array.00000`.

The canonical pack producer is `Tools/packaging/prebuild-runtime.mjs`.
It accepts only the checked Jet runtime object and explicit C platform
archives. It verifies the object/pack symbol inventories and records
`rustObjects: 0` and `llvmObjects: 0` only after successful production.
These records still require the admitted producer execution and its trace;
generator ownership metadata alone is not a Rust-free runtime proof.

`Source/Main.jet` provides the native normal-entry `jet_rt_main` boundary;
the seed CAbi copy is excluded by `cfg(not(jet_native_pack))`. The host must
capture argc/argv/envp before entry. Main validates the single logical Env
seed, initializes native TLS and registries, starts Observe exactly once,
and invokes the checked System V `void(void)` entry through `os_call0_unit`.
This raw entry is not a Jet closure header. The process boundary has one atomic
claimant and one pinned 32-byte state (winning tid/code and reusable output
scratch). Normal return runs reverse-order exit handlers, obtains the parked
report, drains the scheduler, and flushes the real stdout/stderr streams.
It releases the claim before returning zero so a loaded-image host can enter
again; a parked-report status exits the process instead. Its I64 result carries
the existing C int32 status in eax; the loader normalizes those low 32 bits.
Fatal headers claim and clean up before emitting their first diagnostic byte;
footers flush and exit with the winning status. Explicit exit uses the same
`jet_rt_process_exit` boundary. Neither fatal nor explicit exit drains or joins
workers, avoiding a worker waiting for its own in-flight scheduler count.
A concurrent loser parks until the winner's exit_group; a reentrant winner
exits immediately without another report. OOM abort and unreachable/101
host-fault paths remain independent. `Tests/run-main-fixtures.mjs` requires an
explicit fresh native pack and covers entry reuse, reverse callbacks, real
parked/worker/concurrent stops, stream byte order, and accepted Observe restart
versus disabled process-once report guards.

Controlled clocks use the native World128 state in `Source/World.jet`.
The active capability is borrowed at TCB offset 104; zero means no World.
`world_wall_ms(out)` and `world_monotonic_ns(out)` return presence separately
from the raw I64 sample, so a controlled epoch of zero never falls through to
the host. World state survives task boundaries and ends only with its owning
World lease. Wall time is the saturating sum of the fixed origin and virtual
monotonic milliseconds, matching the Rust seed.

MemSentry is native in `Source/Sentry.jet`, with the same R0801/R0802/R0803
diagnostic rows and observational 4-MiB JSONL ledger as the Rust-tier seed.
Gate/fence saves remain caller-owned 64-byte frames; the saved native gate is
56 bytes at TCB128..184. Frame-owned raw ranges expire at the frame boundary,
not at a gate boundary, and the newest expired registration determines whether
a dangling address refers to a Jet frame or quarantined heap storage. Native
packing excludes the five former Rust pointer/scope exports. Ledger failures
never alter the delivered fault.
Raw-addressed native functions hold a distinct sentry allocation-frame token
through normal and stopped cleanup; the gate save is not its lifetime owner.
An instrumented raw read skips its native load after a delivered fault and
leaves its payload unwritten for the MIR Propagate cleanup edge.
Shield defers cancellation/deadline/interrupt-class stops but never suppresses
a panic or runtime fault; both Check and Propagate expose those immediately.
Raw Deref is a can-stop MIR operation. Its Propagate is emitted before the
result is bound into a lexical cleanup frame, including inside Shield; MIR
legality requires all result reads, projections and drops to be dominated by
the resume edge. Cleanup never reads or drops an unwritten raw-read result.
`run-world-sentry-fixtures.mjs` drives real controlled-thread timer ordering,
all four MemSentry seed unit witnesses, a trivially copied inline Source read
whose stopped frame is observed as released, and byte-for-byte JSONL parity
with `jet_mem::jet_memory_ledger_record` from the freshly composed Rust seed.

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
The all-parts adapter refreshes the compiler-call process boundary using its
stable `JET_NATIVE_COMPTIME_ACTIVE` declaration, not an explanatory comment.

### Native UI node ABI

`Compiler/JetBackend/Source/UiABI.jet` defines the canonical 160-byte native
node layout in one table, included by both compiler and runtime manifests.
It is not the layout of a legacy Rust UI allocation and must never be used
to reinterpret one. Callback slots are raw native `SendFn` closure pointers,
not registry IDs or general `Fn` values. The transfer adds three private
logical arguments: IME mode, click closure, and drop closure. A present IME
has mode 0 (Native) or 1 (Disabled); absent appended words are ignored.

Native node cloning allocates a distinct node, clones its owned text and
children, and atomically retains each callback once for the new node.
`UiNode.jet` owns the raw transfer, cloning, destruction, equality, three
borrowed field getters, and the existing accessibility/shortcut transfer
symbols. Callback ownership uses the already-atomic `ClosureRefs.jet`
exports from `feaec0860`: retain publishes another owner, and unref's
last-owner result alone authorizes invoking the generated release helper
at offset 16. That helper drops captures and frees the closure allocation,
as in `Lower/Closures.jet:445-455` at `27a73d5c9`. It runs outside locks.
No separate SendFn refcount mechanism or exports are introduced.

The private typed callback-call override remains an admission requirement;
ordinary `Fn` values and Rust callback registry IDs cannot enter these native
slots. E4 text and exact Int imports must resolve to their native Jet
providers. Metadata clone/drop/equality is now Jet-owned in `UiNode.jet`.
`UiABI.jet` single-sources Accessibility40 (optional name/description E4
pointers, length/capacity/data), State16 (tag/payload), and Shortcut16 (owned
E4 key pointer/modifier bits). Tags match `HostServices.rs:1617-1624`:
Disabled=0, Busy=1, Expanded=2, Checked=3, Selected=4, Required=5, Value=6.
Only Value owns a text payload; unit variants ignore their unused words.
Clones allocate independent headers, state buffers and text. Equality follows
Rust's field/variant/ordered-vector value equality, not pointer identity or
capacity. Constructors must provide these native carriers, never Rust boxes.
The six interim metadata ledger rows retire with this provider cutover;
source/C syntax checks alone still do not admit a native UI pack.

`UiNodeWitness.c` links the real native providers. It checks all 160 bytes'
field contracts for absent and populated transfers, poisoned ignored words,
borrowed getters, distinct cloning, callback invocation, and cross-thread
clone/drop. Host pthread workers explicitly attach/detach the native TCB;
they never reuse an inherited GS base. Two threads additionally perform 100,000 retain/unref pairs
each on one shared closure before its single final destruction. Its counted
closure allocations and frees use the real Jet allocator, not a replacement.
Source parsing and C syntax checks are not native byte-parity evidence.

`UiMetadataWitness.c` additionally covers all seven state variants (including
Expanded true/false and unequal Value text), optional name/description,
ordered-state/length equality, normalized shortcut key/modifier equality,
independent nested clone allocations, and node metadata transfer/clearing.
Link the real composed runtime object with `-Wl,--wrap=jet_rt_free`: the wrapper
only observes frees and always forwards to the real allocator. Every watched
metadata header, state buffer, text header and text buffer must be freed once.
The adjacent stdout golden is expected output until a composed native run
observes it; it does not grant execution or public Core UI census credit.

### Native display runtime

The allocator suite has a named `alloc_tests` entry; its library unit never
declares a program `run` entry. C exports have no failure-row annotation.

The C ABI route generator reads every canonical `CAbi*.rs` except generated
`CAbiRoutes.rs`, including unsafe C exports. Hand-owned exports do not get
generated adapter bodies; canonical signatures still supply ownership checks.

### Native arena resource counters

`MemRegistry.jet` is the single native owner of the seven memory observation
words from `Prelude/Observe.rs`: outstanding arenas, allocations and bytes,
retained block capacity, high-water bytes, and the two allocation-probe totals.
Its priority-1 runtime initializer allocates its own state after the heap is
ready; it does not share allocator policy cells or count ordinary heap blocks
as arenas. The provider uses the existing `Sync.jet` atomic leaves.

Core's Arena/Bump/Fixed/Pool implementations call the open/alloc/reset/close
hooks, and their RawBlock owners call retain/release. Zero-sized values count
as allocations with zero bytes. Counters carry unsigned machine-word bits,
wrap on fetch-add/subtract, and snapshot independently with relaxed loads,
matching the reference rather than promising a transactional metric snapshot.
The high-water candidate alone saturates on unsigned overflow. Probe reset
and take use sequentially consistent exchanges and never clear live facts.

`jet_rt_mem_resources(out)` writes three words (arenas, allocations, bytes)
into caller-owned 24-byte aligned storage for the Observe live writer.
`MemRegistryWitness.c` links the real native provider and covers lifecycle,
zero-size, unsigned wrapping, probe isolation and concurrent allocation totals.
Compile it with `cc -pthread` against the composed runtime object; its success
line is `MemRegistry: lifecycle, zero-size, probe isolation, unsigned wrap,
concurrent totals ok`. Source/syntax checks do not establish native parity.
### Native display runtime

`Compiler/JetRuntime/Source/Display.jet` owns the scalar C display leaves.
The generic `jet_fmt_display`, `jet_fmt_debug`, and `jet_fmt_show` calls are
resolved by checked type in native lowering, not erased into untyped runtime
functions. Text results use the shared E4 `{ptr, len, cap}` UTF-8 carrier.
Exact Int formatting borrows the canonical base-1e9 limb carrier; fixed
signed/unsigned words never enter arbitrary-precision arithmetic.

Today's `JetDisplay`, `JetShow`, and `JetDebug` implementations for binary64
all use Rust's shortest Debug spelling: signed zero is `-0.0`, integral
decimals keep `.0`, exponent notation applies below `1e-4` and at/above
`1e16`, and NaN ignores its sign and payload. `FloatShortest.jet` reuses
the pinned Dragon digit algorithm, with one bounded workspace released
before returning. Rust-tier scalar C bodies remain seed/verification paths
under `cfg(not(jet_native_pack))`; they do not define symbols in native packs.

Binary32 has two raw-word C leaves, `jet_rt_float32_to_string(bits: I64)`
and `jet_rt_float32_debug(bits: I64)`, returning the same owned E4 text.
Only low32 is interpreted; upper32 is ignored. Its 24-bit decoder (bias
127, subnormal unit 2^-149) feeds the shared bounded Dragon core, without
widening to binary64. Display follows Rust f32 `{}`: fixed decimal, `-0`,
and no mandatory `.0`. Debug follows Rust f32 `{:?}`: `-0.0`, integral
`.0`, scientific notation below f32(1e-4) or at/above f32(1e16). Those
binary32 bounds are `0x38d1b717` and `0x5a0e1bca`; the binary64 bounds
are not reused. Rust 1.90 `core/src/fmt/float.rs` and
`core/src/num/flt2dec/decoder.rs` define these spelling/decoder rules.

`FloatBits.jet` contains ordinary Core-backed reference functions. Only the
runtime-object driver may replace their validated checked IDs with internal
LIR `Bitcast` operations; normal program lowering executes their bodies.
The same private table handles the thread-entry address relocation. Its
signatures reject generics, captures, failure carriers, writable parameters,
and mismatched scalar types. Bitcasts preserve all 64 bits and require
trivial Integer/Float/Pointer layouts; they are pure operations, not optimizer
copy aliases. There is no MIR, language, or Float-assembly extension.

The optional ABI witnesses are selected with `run-runtime.mjs OUT --test
FloatBits`, using the shared single-fixture selector. Their additional export
list is confined to that test object, never the production runtime manifest.

`Compiler/JetRuntime/Tests/run-display-goldens.mjs` prepares all binary
exponents with mantissa edges, decimal-power neighbors, signed zero,
NaN payloads, infinities, subnormals, one million deterministic random bit
patterns, decimal ties, integer transitions around 2^53, and every floating
literal in the registered golden corpus. Its freestanding
client calls the real Jet exports in the native pack and compares both
float leaves byte for byte with a separately compiled Rust Debug oracle.
It also checks scalar limits, UTF-8 boundaries, the boxed Int limb layout,
Jet-owned exports, and that every Jet-object import resolves in the pack.
The binary32 differential additionally covers every finite exponent with
mantissa edges, decimal powers, threshold neighborhoods, decimal ties,
2^24 integer transitions, signed zero, subnormals, NaN payloads and
infinities, upper32-poisoned edge words, and one million deterministic
random32 samples. The real native C witness compares both leaves against
independent Rust f32 Display/Debug bytes; `binary32_byte_equal` is recorded
separately from binary64 parity.
The same runner executes the selected passing print/format goldens against
that pack and records a one-million-float formatting cell for native Jet and
the Rust reference, including a digest comparison and measured timing ratio.
The runtime-object path currently has no optimizer; its timing is labelled
“runtime object unoptimized” and is not an O1 performance verdict.
These checks are behavioral proof only when executed against the composed
runtime object and pack; corpus preparation alone is not a pass.

### Unit-test runtime in Jet

`Compiler/JetRuntime/Source/Test.jet` owns native unit outcome classification,
lossless outcome wire encoding, and text/JSON reporting. Its opaque outcome
contains four machine words: flags and owned E4 name/stdout/stderr handles.
Wire records retain the producer protocol:
`flags name_bytes stdout_bytes stderr_bytes -\n` followed by those three UTF-8
payloads. The producer, not the runtime reporter, owns test selection/order
and the `JET_NATIVE_TEST` framing. Capture policies and summary counts retain
the canonical `TEST_PRELUDE` behavior, including expected failures, unexpected
passes, skips, control-character JSON escaping, and full-width shuffle seeds.

Only the native runtime archive uses `jet_native_pack`: its test adapters
transfer owned E4 text to Jet outcomes and hold outcomes opaquely across task
spawn/join/cancellation. Ordinary Rust-tier harnesses retain their Rust
outcome and reporting bodies. The native reporter's raw environment-presence
import belongs to the remaining host layer; presence must not disappear when
an environment value is not UTF-8.

`TestCapture.jet` owns per-thread capture over the scheduler's generic TLS
key mechanism. Its idempotent `.init_array` initializer runs at priority 3,
after thread initialization at priority 2. A private module global holds the
process-lifetime key. Its one 136-byte per-thread state owns the capture
builder/tagged frame list and the `Core.rs:678-860` test cells:
expectations at 32, completed expectations at 56, unmet at 80, timeouts at 88,
whole-skip at 112, and failure message/detail at 120/128. No second TLS key
or reserved TCB test slot is introduced. `TestScope.jet` implements the
matching expectation, timeout, skip and failure APIs; all stored text is E4
owned, borrowed inputs are cloned, and take transfers ownership. Timeout
samples use host `CLOCK_MONOTONIC`, like Rust's `Instant`, even inside World.
Nested begin/take operations preserve outer captures. The TLS destructor
reclaims abandoned captures and every test cell on thread detach. Compiler
print syntax appends bytes and one LF while capturing; raw `stdout.write`
bypasses capture, as it does in the Rust test prelude.

Invocation still depends on L8a-2's task-control/stop-record helpers.
Replacing `jet_rt_test_unit`'s remaining Rust `jet_test_run` boundary requires
installing an inner control, taking its diagnostic, cleaning up capture,
and restoring/propagating the parent's cancellation or deadline. The callback
environment is borrowed; the outer task adapter releases it exactly once.
Property generation remains a named gap, not an emulated or empty engine.
The unit-only wire consumer refuses property records.

`Compiler/JetRuntime/Tests/test-unit-runtime.c` links against the composed
Jet object and fresh remaining native archive, exercising ownership, flags,
and wire round trips without duplicate test providers. Capture fixtures cover
32-level nesting, detach cleanup, and eight pthreads with isolated output.
The `scopes` case covers borrowed-input ownership, replacement/take/clear
failure pairs, first-stop-wins, nested/duplicate/zero expectation IDs,
ordinary-failure exclusion, completed precedence, parent stop reset,
timeout-depth pruning, skip reset, and full detach cleanup across eight
pthreads. `TestScopeTests.jet` in the World/Sentry fixture unit additionally
witnesses mismatch/unmet publication into the real task stop record and
Instant saturation/backwards-clock arithmetic. These fixtures require the
fresh composed object; syntax/parse receipts alone do not prove behavior.
`test-unit-runtime.mjs FIXTURE_EXE` compares exact status/stdout/stderr across
capture policies, text/JSON reports, empty reports, UTF-8/control characters,
shuffle boundaries, and malformed/property records. Runtime admission also
requires native `jet test` checks on `features/basics/onboarding/run.jet`,
`features/tooling/test_members.jet`, and `features/tooling/expected_fail.jet`,
across all capture, filter, serial and shuffle policies.
`run-test-runtime.mjs OUT JET_ONLY_RUNTIME_OBJECT HOSTED_JET`
checks the object's audited undefined-symbol set, builds/runs that fixture,
and runs all eighteen native CLI cases. Standalone C witnesses link the full
Jet-authored `KEY/jet_runtime_jet.o` once plus platform libraries; aggregate
fixture objects link alone. Live Rust-provider/oracle modes are parked:
Rust reference retired (owner #38/#59); Tower #4923. Existing frozen expectations
remain usable; retirement does not remove any Jet or C case.
Display preparation retains the complete corpus; execution reads saved oracle
captures with `--frozen DIR` rather than compiling Rust. HTTP body performance
runs all four Jet trials with optional frozen JSON timing comparison. World/Sentry
fixtures link their aggregate object alone and may compare a saved ledger via
`JET_SENTRY_LEDGER_REFERENCE`; no reference rlib is built. Host native goldens
keep authored expected streams, while only the live reference mode is parked.


### Raw file and process host boundary

`JetRuntime/Source/HostFiles.jet` owns the Linux descriptor transport. Its
canonical FD72 header has the futex mutex at 0, descriptor at 8, closed flag
at 16, owned E4 path at 24, buffer at 32, capacity at 40 (negative for
writes, zero for raw readers), filled bytes at 48, position at 56, and refs at 64.
Explicit close affects aliases; last release frees the state. Portable stat
facts hide the kernel's stat layout, and statx reports birth-time availability
instead of assuming it is absent. Held-directory operations never reopen a
source inode or substitute an ambient pathname.

Core owns validation, fault injection and IOError construction. Private leaves
report ErrorOut32 (domain/stage U32, code I64, kind/pad U32, owned E4 message)
and a separate WriteZero fact on write/flush/seek/close. Linux syscall errors
have POSIX domain 1 and zero kind/message. Read and descriptor dup first flush
pending writer bytes; their flush WriteZero error uses custom domain 4, Other
kind and the owned canonical cause because those ABIs have no Bool output.
Reads then make a single kernel read into caller-owned compact bytes, never
consume the write buffer, and preserve Interrupted. A dup owns a fresh empty
buffer and sees the flushed shared kernel offset. Historical
`958047768^:crates/jet-codegen/src/Prelude/CoreLib/JetStd/FileHandles.rs`
owns unbuffered `std::fs::File` descriptors, so private native buffering must
not hide preceding writes from either operation. Its `jet_fs_link_fd_at_raw`
calls AT_EMPTY_PATH once with no pathname or `/proc/self/fd` fallback.
Linux [link(2)](https://man7.org/linux/man-pages/man2/link.2.html) requires
CAP_DAC_READ_SEARCH for that flag and reports ENOENT without it; the transport
returns that fact unchanged. The six-argument process launch
borrows ExecCandidates32 (count, NUL candidate column, script-fallback bit,
searched bit) and Core-prepared argv and effective environment bytes. This
preserves non-Unicode PATH entries without an independent environment. Source
computes the exact reference route's PATH and ENOEXEC policy. The runtime parent
prepares allocations, descriptor copies, limits and `/bin/sh` argv before fork.
The child uses only raw syscalls: stdio mapping, cwd, source-prepared group
setup, SIGPIPE reset, held-directory mapping/chdir, limits/session/PTY setup,
then ordered execve. GNU retry facts remember EACCES, continue past ENOENT,
ESTALE, ENOTDIR, ENODEV and ETIMEDOUT, and stop on all other errors; an explicit
fallback bit alone permits the preallocated `/bin/sh` argv. Child72 has its mutex at 0,
pid/reap-status at 8/16/24, stdin/stdout/stderr/PTY at 32/40/48/56, and refs at
64. Taking a stream transfers an independently owned FD72; terminal input and
output duplicate the master while taking terminal control transfers the
original. Stream aliases may outlive the child. Windows-only PTY control
carriers use capacity I64.MIN and dispatch resize/close to the OS console
handle; Linux and macOS never produce that tag. Native carrier fixtures link
against the complete composed runtime object, not an allocator-only seed.
`realpath` into a Jet-owned fixed 4096-byte buffer preserves exact OS symlink,
`..`, and non-Unicode canonicalization, with the real OS errno on failure;
this ledgered mechanical import is not a pathname-policy replacement.

Owner directive #33 defines an empty exec-candidate set as ENOENT, without an
execve attempt. This is the intentional I9 delta from glibc's oversized-PATH
case that otherwise returns whichever stale errno happened to be present.
Child setup retains its reference error precedence: an invalid cwd or descriptor
still fails at its own stage before the empty candidate set reports exec ENOENT.

The remaining held-file calls reuse this boundary, without exporting a second
set of public-policy adapters:

| Core seed route | Jet raw transport |
| --- | --- |
| `jet_std_files_open_writer` | `jet_fd_open_raw` / `jet_fd_open_at_raw` |
| `jet_fs_directory_close` | `jet_fd_close_raw` |
| `jet_fs_directory_create` | `jet_fd_mkdir_at_raw` |
| `jet_fs_rename_at` | `jet_fd_rename_at_raw` |
| `jet_fs_read_link_at` | `jet_fd_readlink_at_raw` |
| `jet_std_file_reader_stat` | `jet_fd_stat_raw` |
| `jet_std_file_writer_set_mode` | `jet_fd_chmod_raw` |
| `jet_fs_directory_fd_raw` | `jet_fd_number_raw` |
| `jet_fs_stat_at` | `jet_fd_stat_at_raw` |
| `jet_fs_xattr_get` | `jet_fs_xattr_get_raw` |

`jet_fd_stat_at_raw(directory, name, StatFacts128*, ErrorOut32*)` follows
FileHandles.rs:711–758: an empty name stats the held directory; a final entry
opens with `O_PATH|O_NOFOLLOW|O_CLOEXEC`, stats the owned descriptor, and closes
it, including stat failure. The source resolves/validates intermediate names.
Ambient path stat continues to use `jet_fs_stat_raw`, not a fake directory.
`jet_fs_xattr_get_raw(path, attribute, compact_bytes, length, ErrorOut32*)`
performs one `lgetxattr` syscall, returning the byte count or -1. A null buffer
with zero length queries size. FileHandles.rs:988–1031's absent/unsupported
mapping, allocation and ERANGE retry remain source policy. Descriptor numbers
are borrowed facts: source retains the directory until the consuming leaf
duplicates it; they are not independent owned handles.

### Native shared process pipe holder

`HostPipe.jet` owns `Pipe24`: non-atomic reference count at 0, RefCell mutable
borrow state at 8, and an owned reader FD72 pointer at 16 (`0` means `None`).
The source facts are `CommonTypes.rs:162–165`'s
`Rc<RefCell<Option<BufReader<ProcessReader>>>>` and `Process.rs:3325–3343`'s
shared mutable buffered position. Clone returns the same holder and increments
its count; only the last drop releases the FD72 and frees the holder. The FD
is taken from `jet_process_take_stream_raw`, not borrowed from or retained
through Child. Shared close takes the optional reader, so every clone sees EOF.
No Rust Box/Rc/RefCell offset and no independently duplicated fd is involved.

The four `jet_rt_handle_{clone,drop}_process_{stdout,stderr}_stream` exports use
the canonical handle symbol convention. `jet_process_stream_line_bytes_raw`
borrows the holder and returns `Result<[U8], IOError>` as a native tag plus
owned compact data/length or an owned error. It shares FD72's refill buffer,
includes LF, preserves malformed UTF-8 and bare CR, and retries Interrupted
as `BufRead::read_until` does. Checked Core `__stdout_next_line` and
`__stderr_next_line` own empty-byte EOF, UTF-8 decoding and the one-LF/one-CR
terminator policy. No fixed host next-line iterator substitutes for them.

When TAKE supplies a writer-mode FD72, the first buffered read flushes pending
output under the FD lock before converting its buffer to reader refill mode
and resetting both cursors. Flush failures leave the writer suffix intact and
return through the fallible read edge (TAKE itself has no error result).
Writes through retained reader-mode aliases bypass refill storage and use a
kernel write, so they cannot corrupt unread lines and a pipe read-end write
returns EBADF.

A conflicting mutable borrow panics with `RefCell already borrowed`, matching
the seed's observed rustc 1.97.1 commit `8bab26f4f` and that commit's
`library/core/src/cell.rs:902,912–917`. Its trap path does not allocate.
`Tests/process-pipe.c` plants real-descriptor, shared-refill, interleaved-clone,
shared-close, actual comptime-stop release and signal-reentrant borrow
witnesses. Execution and allocation-ledger-zero remain unexercised; the latter
requires the ProgramAllocator counting/facts provider, not a fake allocator.

The native Shared transaction log retains each participating cell and runs each
edit callback once against a private working value. Nested commit transfers
the child's stage to its parent; outer commit acquires all permits in physical
owner order, prepares every clone and revision, then publishes all participants
before releasing permits. The commit clone is observable and matches
`MathTaskMem.rs`'s commit hook. Failure disposes prepared stages without
publication. The process-wide outer gate covers begin through commit or
rollback; generated scope/drop glue must close that authority on every exit.
The actual-kernel clients are `JetRuntime/Tests/StmTests.jet` and
`sync-fixture.c`, assembled separately by `run-channel-fixtures.mjs`
(`assemble-stm`, `link-stm`, `run-stm`).
The dropped-root witness holds an uncommitted staged edit while a second
attached pthread attempts begin; rollback must let that thread enter and see
the old published value. Its subsequent commit advances exactly one revision.
Together with the compiler's generated early-return `MovePlace`/rollback
`Drop` fixture, this covers the uncommitted-root gate-release obligation.
The witness is source evidence until the assembled native fixture is run.

Condition wait registers its FIFO ParkSlot before releasing an edit permit.
Notify-one consumes the oldest registration; notify-all drains the queue;
notifications without waiters become pending notifications for later waits.
The owned guard's mutable token records whether reacquisition succeeded:
`cell` is held and `cell|1` is unheld but still retains the cell. An unheld
access delivers the named invalid-guard stop; dropping it releases only its
retained owner, never another writer's permit or revision. The wait callback
writes a single Bool byte; its error output transfers one E4 String owner only
on a non-stopping error return. `assemble-sync`, `link-sync`, and `run-sync`
include the Condition queue and cancellation-cleanup witnesses.

### Remaining native String routes

`TextRoutes.jet` supplies the C route ABI for chars, copy, byte count, lines,
repeat, replace, both explicit slice forms and ASCII case conversion, reusing
E4 ownership, UTF-8 scalar decoding, constant-storage substring search and the
existing character-range failure report. Chars/lines return an owned word
column plus its count, with null for empty columns, matching generated
`list_out`. Byte count returns the canonical exact-Int word. Lines match
Rust `str::lines`: remove LF and its optional preceding CR, preserve bare CR,
and omit the extra line after a final LF. Replace uses nonoverlapping matches;
an empty pattern inserts at each Unicode-scalar boundary including both ends.
Repeat clamps negative counts to zero and copies into one exact allocation.
ASCII casing leaves every non-ASCII byte unchanged. Slice bounds are character
indices and exclude the end, including an empty slice at the character count.
The generated route exporter removes replaced Rust definitions while retaining
shared carrier metadata; no second String carrier is introduced.

The x64 runtime-pack census excludes wasm32-only allocation exports (WebAbi,
font/HarfBuzz scratch, numeric Web buffers and Web D-FRED). Those U32 linear-
memory/provenance/TLS contracts remain separate target rows, not native-pointer
casts. Date/time formatting and Pool construction are Core source-owner rows,
not String/allocator runtime aliases. The private image-runtime stop/String
allocator and L1/L3 canonical allocator/split providers are existing Jet bodies.

### Native Dyn ownership boundary

`JetBackend/Source/DataABI.jet` single-sources canonical Dyn's raw variant
words and payload offsets; these are never a Rust allocation layout.
The `JetRuntime/Source/Dyn.jet` provider retains private `data_tree_*`
helpers for generic storage, not a second public type.
All exported lifecycle, constructor, getter and sequence symbols use `Dyn`;
obsolete `DataTree` exports and `TypedText` are removed.
Constructors clone borrowed text and exact Int owners, consume sequence
holders, and preserve ordered Object entries, including duplicate keys.
Int getters clone the retained exact owner rather than narrowing to i64.
Sequence getters lend children and clone keys; text getters clone owners.

Equality is canonical Dyn's ordered, variant-sensitive equality, including
IEEE Float equality (NaN unequal even to itself, signed zeros equal). Drop
releases owned Int, text, byte, date-time and recursive sequence payloads.
Debug renders compact JSON; show/display first use scalar `to_text`
projection and otherwise JSON. Number preserves its lexeme; Bytes renders
standard base64; DateTime retains instant, nanos, leap label and frozen offset.
`CompilerNeededJSON.jet` ports the canonical shared strict JSON parser with
exact-number mode: every numeric token becomes Number until a typed read.
Error reasons and the JSON/Syntax, offset-zero, line-present, column-absent
adapter remain unchanged. This is not Core's separate policy parser.

`DynNative.jet` marshals the compiler's checked enum through the descriptor
in `Lower/Dyn.jet`, rather than assuming source/runtime tags or element
offsets coincide. Both directions clone borrowed owners; packed byte lists,
padded Object entries and recursive payload wrappers follow descriptor facts.
Compiler JSON does not construct DateTime. Main explicitly scoped that
bridge variant out of cutover: it raises the registered E3001 panic
`Dyn DateTime is not yet bridged natively`, never a silent ZonedDateTime
reinterpretation. The complete ZonedDateTime projection requires a separate
schema extension after cutover.

`JetRuntime/Tests/DataTreeWitness.c` retains its existing filename but uses
only canonical Dyn exports. It requires a real Jet object and
`--wrap=jet_rt_free`, checks owned clones and exact-once frees, and its
`exact-int-large` case accepts `i64::MAX + 1` without narrowing.
`CompilerNeededJSONTests.jet` and `DynNativeTests.jet` cover parser/outslot
contracts and source-layout-independent roundtrips. Execution of these
fixtures is still required; source inspection is not native execution proof.

### Compiler-needed scalar runtime routes

`CompilerNeededMath.jet` provides all 37 compiler-needed math routes.
Transcendental, rounding, remainder, power, hypot and fused multiply-add
operations call the platform's matching C libm entry points, with no Rust
provider or approximation. Absolute value and predicates preserve IEEE bits;
comparison retains the Rust NaN ordering; sin/cos keeps tuple order.
Existing audited FloatBits intrinsics supply bitcasts, with native exact Int
owners at route boundaries. The C fixture compares result bits against the
same platform libm, including payloads, subnormals, signed zeros and fusion.

The three text predicates stream the existing E4 UTF-8 decoder over pinned
Unicode 17 properties; empty input remains false. Alphabetic/Numeric
balanced tables regenerate from checksum-pinned UCD inputs with frozen range
hashes, not Rust source or a locale-dependent libc predicate.
`CompilerNeededURL.jet` ports the kernel's parse/normalize contract, including
its permissive host/bracket rules, Punycode, percent decoding, query ordering
and error precedence, and provides the existing JetURL record lifecycle.
Provider fixtures are Jet-only; no frozen Rust implementation is rebuilt.


### Shared record-variant panic parity

Dyn, DataEvent, TLS, TaskFailure, and ProcessResourceLimit variant checks
call the single internal `rt_record_variant_panic()` in
`JetRuntime/Source/Trap.jet`. It materializes the seed's 33-byte reason in one
owned buffer and calls the existing `jet_rt_panic_message`; after task
publication it frees that temporary buffer. At root, header + message +
`trap_msg_prelude_end()` + footer remain byte-identical to
`jet_panic("<core.prelude>", 0, "record variant index out of range")`.
`trap_msg_prelude_end()` is the message terminator plus
`trap_context("<core.prelude>", 0, empty function/source)` bytes.

The seed reason is `Core/CAbi.rs`'s `record_index_stop` message. `Core.rs`'s
`jet_panic` and `jet_arithmetic_stop` both enter `jet_runtime_stop`;
`jet_runtime_stop_unwind` carries the raw message into the task runner.
`Scheduler.rs`'s `jet_scheduler_panic_message` strips the rendered report and
source location; `Observe.rs`'s `jet_observe_task_failure_message` prefixes the
canonical task identity to produce the joined child `.Panicked(reason)`.
The native `trap_simple` and record-variant specialization use the same
`rt_task_panic_publish` transport. Runtime-private callers return neutral
stopped values before reading or cloning a mismatched payload; compiled
consumers must propagate the checked Panic fact before using those values.

### DataTree sequence foreign-stop parity

The List/Entries `_get` exports follow
`CAbiRoutes.rs:8240-8241,8253-8254`: `vec[index as usize]` is a plain foreign
Vec panic under guard, not a language `#require` stop. `data_sequence_slot`
calls the shared `rt_seq_index_oob_stop(length,index)` from held
`c39d89b6b` before computing a data pointer or cloning an entry key. The
helper flushes stdout, prints the exact bounds body with the index as u64,
and exits 101 without task publication, diagnostic footer or atexit.

`data_sequence_new` rejects negative capacity and capacity times the native
stride exceeding i64::MAX before multiplication/allocation. Growth at the
same byte-capacity limit follows the same shared
`rt_seq_capacity_overflow_stop()` in Trap: flush stdout, raw
`capacity overflow\n`, exit 101. This replaces the former language overflow
stop and is shared by all native List/Entries constructors.

The C witness has eight List/Entries modes: index0/index-1 on an empty list,
capacity-1, and capacity `i64::MAX / stride + 1`. The three
`DataSequence*.stderr` files are canonical expected bytes;
`Tests/run-data-sequence-stops.mjs OUT DATA_TREE_WITNESS` requires empty stdout,
exact stderr and exact exit101 for each mode. These native process cases
have not been executed. The composed source unit must include the index helper
from c39d89b6b as well as the capacity helper before caller/import admission.

### Native encoding error ownership boundary

`JetBackend/Source/EncodingABI.jet` single-sources the private enum8, Cause32
and Error80 carriers. `JetRuntime/Source/EncodingErrors.jet` supplies each
complete HAND family for EncodingFormat, EncodingErrorKind, EncodingCause
and EncodingError; it does not supply generator-owned DERIVABLE routes or
EncodingLimits. Enum handle operations use pointers; record enum fields use
raw declaration-order tags, checked against EncodingTypes.rs:29-61.

The C ABI follows `CAbiRoutes.rs:9394-9466`: constructors clone borrowed
texts and range-convert borrowed exact-Int inputs to raw i64 before allocation
or consuming any payload. Option Int inputs lend a nullable pointer-to-word;
getters publish fresh exact owners only when present. Error construction
consumes the optional Cause handle, unlike its borrowed Text arguments.
Clone recursively duplicates text and cause owners; drop releases each once.
Equality follows EncodingTypes.rs:63-80 field order, ignoring absent payload
words but including the cause snapshot.

Show/display/debug share the exact `Core/EncodingError.rs:8-28` projection,
as delegated by `JetStd/CommonTypes.rs:182-217`: format, kind and byte offset,
independent optional line/column, nonempty path, then reason; cause is omitted
from render but not equality. The existing text builder and signed i64
formatter are reused, including negative offsets and i64::MIN. Invalid enum
ordinals use the same native-range stop as CAbiRoutes.rs:11-13.

`JetRuntime/Tests/EncodingErrorsWitness.c` is a real-object client, requiring
`--wrap=jet_rt_free` with asserted interception and exact-once recursive frees.
It exercises all exported families, constructor transfer/borrow ownership,
fresh getters, deep-clone independence, every equality field, absent versus
present-zero options, both i64 edges and all format/kind render combinations.
The separate `format-range`, `kind-range` and `native-overflow` modes require
the canonical native arithmetic range diagnostic. The expected normal output
is `EncodingErrors ownership/equality/render OK`. Native execution remains
unexercised until the integrator composes the fresh green E2 FINAL runtime;
parser and strict C11 syntax receipts alone do not establish runtime parity.

### Memo statistics, mail limits and watcher C ownership

`MemoABI.jet`, `MailLimitsABI.jet` and `WatchABI.jet` define native layouts,
not Rust layouts. Their runtime providers preserve the field types in
`Prelude/Memo.rs:74-80`, `CoreLib/Email.rs:156-164` and
`CoreLib/JetStd/CommonTypes.rs:640-665`. The C boundary borrows exact Int
arguments, converts to raw i64 before allocation, and returns freshly owned
exact Int getters (`CAbiRoutes.rs:10703-10728,11668-11705,12158-12195`).
Borrowed text is cloned on construction, getters and handle clone; drop
releases each owned text and carrier. Watch enums are native ordinal words;
event construction range-stops invalid domain/kind ordinals. WatchDomain's
separate generated glue is not duplicated by this provider.

Equality follows the Rust derives in field order. WatchEvent's custom show
(`CommonTypes.rs:6772-6777`) deliberately omits pid/port and emits unquoted,
unescaped path/detail, unlike a derived debug representation. It uses the
existing text builder. `RecordGlueWitness.c` links the real Jet object with
`--wrap=jet_rt_free`, asserts interception, clone/getter independence,
exact-once destruction, per-field inequality and enum/render goldens. Its
invalid-ordinal and exact-Int overflow modes exercise the established stops.
Native execution remains unexercised until composed source-root ownership
admission and generator ABI validation; syntax checks alone do not prove it.

### Plugin records, log spans and asynchronous policy carriers

`PluginABI.jet` defines inline 24-byte frames in the existing native List24
header, and native faults owning that header. Frame sequence push moves the
frame's text owners into inline storage and frees only its source shell;
sequence get lends inline storage, never an independently droppable frame.
Fault construction consumes the sequence holder; clone recursively copies its
text owners and drop releases its children, buffer, holder and fault once.
These contracts mirror `CAbiRoutes.rs:8496-8505,11755-11797` and the ordered
derive equality in `jet-foundation/src/PluginWire.rs:28-43`.

`LogABI.jet` stores LogSpan's raw i64 and owned name
(`CommonTypes.rs:2704-2708`, `CAbiRoutes.rs:11738-11751`). `AsyncPolicyABI.jet`
stores raw i64 capacity and overflow ordinal, with separately owned ordinal
handles for overflow. Codec construction checks the ordinal but intentionally
does not call policy validation: `CAbiRoutes.rs:9545-9558` permits zero and
negative capacities; `ReactiveEventWatch.rs:808-841` distinguishes construction
from validation. Exact Int arguments are borrowed and converted before
allocation; Int getters return freshly owned exact values.

`PluginPolicyWitness.c` requires real Jet exports and asserts free wrapping
actually ran. It covers shell consumption, List growth, lent getters, recursive
clone independence, exact-once destruction, ordered field/sequence equality,
i64 edges, all unit enum values and invalid-ordinal/index stop modes. Its
stdout fixture is an expected result, not an execution receipt. Native
execution remains unexercised pending pack composition and ownership admission.

### Exact borrowed String debug rendering

`TextDebug.jet` appends the Rust reference's `str` debug representation into
the existing native text builder, borrowing a distinct E4 UTF-8 input. It
quotes the result, escapes NUL/tab/CR/LF/backslash/double quote, leaves single
quotes unchanged, and emits lowercase minimal `\u{hex}` for Grapheme_Extend
or non-printable scalars. Combining marks are escaped at any position.
Plain runs append without temporary allocations; this is not JSON escaping.
The shared core also serves `jet_rt_require_debug_string`, retaining Jet's
D-STR-ESCAPE1 NUL spelling `\u{0}`; Rust-derived debug uses `\0`. Both modes
reuse the existing `trap_scalar`, `trap_debug_short` and `trap_debug_escaped`
helpers and canonical `TRAP_DEBUG_RANGES`, rather than a second decoder/table.

The reference is rustc 1.97.1, commit
`8bab26f4f68e0e26f0bb7960be334d5b520ea452`, with Unicode 17.0.0:
`library/core/src/fmt/mod.rs:2916-2960`,
`library/core/src/char/methods.rs:474-487`,
`library/core/src/unicode/printable.rs` and
`library/core/src/unicode/unicode_data.rs::grapheme_extend`.
The source tarball SHA-256 is
`622c2b429c53cbfdc0dd3a51d03554e91cd63ebec1912c1f5709640cdfef1a9d`.
`Tools/agent/gen-text-debug-tables.mjs` checks both Unicode input checksums
and compares their exact classification with the existing Trap table;
it emits only C witness goldens, never a second runtime table. Generation is
offline and normal builds need neither Rust nor network access.

`TextDebugGoldens.h` contains 36 expected vectors produced by that exact
rustc's `{:?}` for `str`, including embedded controls, combining marks at
start/mid, ZWJ, soft hyphen, BOM, astral printable/unassigned characters,
variation selectors and printability range edges. An exhaustive reference
comparison found zero classification differences on 1,111,936 valid
non-ASCII scalars. `TextDebugWitness.c` links real Jet exports, exercises
borrowed input preservation and repeated builder appends, and requires
free interception and exact-once destruction. Its stdout fixture is expected,
not a native execution receipt; native execution remains unexercised.

### Plugin failure debug and ownership

`PluginError.jet` implements all handle routes for the native carrier in
`PluginABI.jet`. Clone owns an independent recursive fault; drop releases it
once. Equality follows `PluginWire.rs:46-50`: variant first, then Budget's
limit before its fault, ignoring the inactive limit word for other variants.
Display/show clone the message only (`PluginWire.rs:65-68`,
`DBPluginWire.rs:4-11`); debug streams the complete derived enum, nested fault
and ordered inline frame list into one existing builder. String fields use
the shared Rust-debug mode, not Jet's NUL spelling, and raw i64 offsets use
the existing signed-decimal builder helper.

`PluginErrorGoldens.h` contains sixteen outputs of the real PluginWire Rust
derives compiled with pinned rustc 1.97.1
(`8bab26f4f68e0e26f0bb7960be334d5b520ea452`): all variants, all Budget limits,
empty/two-frame faults and text/control/Unicode escapes. The offline
`Tools/agent/gen-plugin-debug-goldens.mjs` packages those oracle outputs for
`PluginErrorWitness.c`. The C client uses valid native allocations, requires
free interception, checks recursive clone independence, inactive-word
behavior, derived equality and separately owned render results. Expected
stdout is not an execution receipt; native execution remains unexercised.

### Shared enum and foreign sequence-stop leaves

WatchKind, PluginLimit and EventOverflow handle glue uses the shared
`EncodingABI.jet` owner through `encoding_enum_clone/drop/eq`; their record
fields remain raw ordinals. Family files no longer duplicate the unit enum
layout or its allocation/equality policy.

PluginFrame's sequence getter preserves `CAbiRoutes.rs:8501`'s foreign
`Vec[index as usize]` failure, calling shared `rt_seq_index_oob_stop` rather
than a Jet language panic. Valid access lends inline frame storage directly.
The foreign stop prints the standard length/index body, interpreting negative
indices as unsigned, flushes stdout and exits 101 without task publication
or atexit. The existing witness stop mode remains unexercised. These callsites
require the shared Encoding and Trap leaves in the composed runtime unit;
source parsing alone does not resolve those dependencies or prove execution.

### Environment, websocket, task failure and SIMD handle families

EnvError and TaskStatus reuse the shared unit enum owner. Their three render
routes preserve the custom text in `CommonTypes.rs:6631-6645,6678-6681` and
`Services.rs:1080-1087,1111-1120`. `WsABI.jet` stores websocket error and
message variants from `WsClient.rs:10-19,57-61`. IO errors own operation text;
MessageTooLarge stores raw i64; Text/Binary/Close messages own their text,
byte list or close reason. Every binary clone, comparison and drop uses the
existing byte-list helpers and `RT_HOST_U8_STRIDE`, never a literal word
stride. Websocket error rendering follows the custom show (`WsClient.rs:22-52`),
including raw operation text and signed limits rather than derived debug.

TaskFailure extends the existing `Task.jet` carrier in place. Its layout is
single-sourced by `TaskFailureABI.jet`; no competing task failure carrier is
introduced. Equality follows `Outcome.rs:2185-2188`. Show/display render a
raw Panicked reason (`Values.rs:454-460`, `TextValues.rs:212-218`), while
debug quotes it with the shared Rust String escaper (`TextValues.rs:221-227`).
Wrong-variant access uses the shared record-variant language panic.

`SimdLaneABI.jet` defines four raw IEEE f64 lane words for F64x4. Clone copies
those words, equality compares lanes in order without a pointer shortcut,
and all renders follow the array-backed debug tuple in
`MathTaskMem.rs:397-423`, using the existing shortest f64 debug formatter.
NaN is unequal even to itself; signed zero lanes compare equal.

`HandleFamiliesWitness.c` requires the real Jet runtime and free wrapping.
It covers all variant render/equality tables, deep text/byte/failure clones,
exact-once destruction, raw i64 edges, inactive payload independence, byte
list empty/nonempty cases and IEEE lane truth tables. Lane render goldens
were produced by pinned rustc 1.97.1's debug tuple/array formatting. The C
byte-list expectation derives from `WS_MESSAGE_U8_STRIDE`, which must match
the composed runtime's canonical one-byte stride. Its wrong-variant mode and
expected stdout remain unexercised; native ownership admission and execution
are not established by parse or C syntax receipts.

### Byte buffer, POSIX path, row policy and event unit glue

`ByteHandlesABI.jet` defines the native ByteBuffer (owned byte-list pointer,
raw unsigned cursor), Path (owned byte-list pointer) and RowPolicy (owned
table/expression Text pointers and the compiled closed-policy ordinal).
These are native layouts, not Rust struct layouts or generated carrier aliases.
ByteBuffer copies the complete byte list and cursor; its three renders ignore
the cursor and print decimal bytes with comma-space separators, matching
`Collections.rs:191-204`. Decimal U8 output creates no temporary Int/Text or
heap scratch. Path preserves arbitrary POSIX path bytes across clone, then
uses the shared UTF8-lossy decoder only when rendering (`PathFiles.rs:6-18`).
The canonical packed byte stride renders directly without staging; legacy
word-stride composition packs through `host_u8_get`. Byte clone/drop uses
the existing stride-aware DataTree helpers, never a private byte stride.

RowPolicy retains the compiled `AllowAll`/`OwnerEqualsUser` form as well as its
canonical text (`Sync.rs:65-68`, `RowPolicy.rs:18-21`). Its two field routes
preserve the actual two-argument, void C ABI in `CAbiRoutes.rs:12832-12840`:
they fill the caller's stack `[data, length, capacity]` descriptor borrowing
the owner's bytes. They do not return fresh Text owners. The descriptor must
never be grown, consumed or dropped; callers may explicitly clone it.

FailurePolicy, DispatchState, EventConfigError and LateEventDisposition
delegate clone/drop/equality to the shared Encoding unit-enum owner.
JetBitSet's five existing native providers in `Collections.jet` are reused
unchanged (external-provider precedent `814254e43`), not duplicated.
All-ref searches found no existing Jet providers for the other new families.
Fraction is intentionally not given a hand carrier: CoreReview4's ruling
retires it to Core-source structural glue on the CoreNumerics7 composition
(`9cea21818` through `4bf243903`).

`ByteHandleWitness.c` requires real Jet exports and free interception. It
covers all 256 byte values, empty/full lists, raw cursor high bits, clone
independence, exact-once frees, arbitrary path bytes including NUL and malformed
UTF8, borrowed policy descriptor lifetime, both compiled policies, every valid
unit-enum equality pair, and reused BitSet sorting/duplicate/negative behavior.
Its byte-list expectations use `BYTE_HANDLE_U8_STRIDE`, which must match the
composed runtime. Its stdout fixture is an expectation, not an execution
receipt. Source parsing and strict C11 syntax do not establish native
ownership admission, runtime linking or execution.

### Native DataLimits and EncodingLimits ownership boundary

`JetRuntime/Source/DataLimits.jet` ports the 19 existing clone/drop/eq,
constructor and field-getter exports. The unchanged C boundary follows
`CAbiRoutes.rs:8838-8869,9470-9507`: borrowed exact Int arguments are checked
through `jet_int_owned_to_i64`, while getters return a fresh exact Int owner
through `jet_int_owned_from_i64`. Overflow uses the shared native conversion's
`native Int argument exceeds host range` stop at `<core.prelude>:0`.

The private native layouts in `JetBackend/Source/DataABI.jet` do not describe
Rust allocations or nominal Core/MIR layouts. EncodingLimits occupies 56 bytes:
raw i64 buffer/depth/item at 0/8/16, optional-total presence/value at 24/32,
and expansion depth/bytes at 40/48. It has no child or Int owners. DataLimits
occupies 40 bytes: an owned EncodingLimits pointer at 0 followed by raw i64
groups/sort/join/output at 8/16/24/32. Its constructor consumes the encoding
handle; clone and the encoding getter deep-clone it; drop frees the child and
then the shell. Equality follows the Rust declaration order in
`EncodingTypes.rs:4-11` and `CommonTypes.rs:797-803`, including optional-total
presence and comparing its payload only when present.
Conversions happen before allocating the destination carrier. The shared stop
is fatal: if a DataLimits scalar argument exceeds native range, its transferred
encoding handle is not freed before process exit.

The optional total constructor accepts a null pointer for absent, or a
borrowed pointer to an exact Int word. Its getter returns raw presence 0/1 and
writes a newly owned Int into the caller's out slot only when present.
`JetRuntime/Tests/DataLimitsWitness.c` requires the real Jet object and
`--wrap=jet_rt_free`. It checks every getter, both i64 edges, absent/present-zero
totals, fresh boxed owners, nested clone/getter independence, equality field
participation and exact-once carrier frees. `encoding-overflow`,
`encoding-field-overflow`, and `data-overflow` independently exercise native
conversion stops. The `.stdout` file is expected output, not an execution
receipt. This provider is held: native caller-layout/import admission and
execution, plus whole-unit suppression of its Rust seed bodies, remain
required before pack cutover.

### Native DataEvent ownership boundary

`JetRuntime/Source/DataEvent.jet` ports the 23 existing clone/drop/eq, tag,
constructor and payload-getter exports, with no additional rendering surface.
Its 16-byte native hand layout in `JetBackend/Source/DataABI.jet` is
`[tag, payload]`, not a Rust or nominal Core/MIR enum layout. The 12 tags follow
`CommonTypes.rs:282-295`: Null, Bool, Int, Float, Number, Text, Bytes,
ArrayStart, ArrayEnd, ObjectStart, Key, ObjectEnd.

`CAbiRoutes.rs:8730-8835` defines the unchanged ABI ownership. Bool normalizes
the incoming u8 to 0/1. Int borrows the exact input and checks native i64 range
before allocating, stores raw i64, and returns a fresh exact owner on get.
Float preserves bits and uses IEEE equality, including unequal NaN and equal
signed zeros; it has no JSON rendering finite-value restriction. Number, Text
and Key clone borrowed strings on construction, clone owned strings on
get/clone, and drop them with the event. Bytes consumes the native List holder;
its getter lends the stored pointer, while clone duplicates its U8 data and
drop frees data then holder. Units store zero payload. A wrong-variant getter
uses the existing panic path with `record variant index out of range`.

`JetRuntime/Tests/DataEventWitness.c` requires the real Jet object,
`--wrap=jet_rt_free`, and `DATA_EVENT_U8_STRIDE` matching the runtime's single
U8 epoch (canonical packed 1). It covers all variants and getters, boxed i64 edges,
fresh-owner getters, Float NaN/infinity/signed-zero equality, independent
strings and byte holders, borrowed byte getters, and exact-once frees.
Separate `native-overflow` and `wrong-variant` modes must stop through the
existing runtime paths. Its `.stdout` is expected output, not a native
execution receipt. Native layout/import admission and whole-unit Rust seed
suppression are still required before pack cutover.

Both C witnesses have `panic-reference` (real `jet_panic` with heap text,
line 0) and `wrong-variant` modes.
`Tests/run-record-variant-panic.mjs OUT DATA_TREE_WITNESS DATA_EVENT_WITNESS TASK_VARIANT_PANIC_WITNESS`
runs the real linked clients serially, requires exact exit 70 and empty
stdout for root stops, saves the direct reference's stderr as the
process-mode golden, asserts message/location, and byte-compares the
wrong-variant output. `TaskVariantPanicWitness.c` additionally spawns and joins
real children calling a nested DataTree text getter on Null, `jet_panic`,
and `jet_rt_numeric_stop`; it checks exact `.Panicked(reason)`, completed
release callbacks, and surviving parent control with no stderr.
Parser/C syntax receipts alone do not prove these execution assertions.

DataTree/DataEvent runtime byte paths use `RT_HOST_U8_STRIDE` and host U8
accessors; their C witnesses default to canonical packed stride1 and keep all
byte indexing/allocation expectations derived from their stride constant.


### Native synchronous Event provider

`JetRuntime/Source/Events.jet` is the Jet-native provider of the synchronous
Event family (D-EVENT1=D), one meaning with `ReactiveEventWatch.rs`
JetEventPolicy/JetEventTrace/JetSubscription/JetEventScope/JetEvent. Its
single-thread shared cells use plain counts: an `Event<T>` handle is its
EventCore (strong/weak counts and the ordered listener list); a Subscription
holds a weak count on the core and its listener id; each listener holds one
count per owner (the core's list and every dispatch snapshot), so a handler
that unsubscribes itself or a later listener keeps its running closure alive
and the skipped listener is never invoked. Dispatch snapshots the active
listeners in priority order (highest first), registration order among equal
priorities; a once listener is consumed before its handler runs; nested emits
dispatch immediately (AllowDepthFirst, the sole policy). A cancelled scope
returns an inactive Subscription and releases the handler unregistered; the
last EventScope handle cancels, unsubscribing every tracked Subscription; the
last Event handle releases its handlers while Subscriptions outlive it. A
handler stop ends the dispatch and leaves the trace null. `emit` holds its own
strong count for the whole dispatch (Rust: `&self` on the `Rc`), so a handler
that drops the last Event handle leaves the core alive until `emit` returns.

The backend (`Lower/Events.jet`) lowers every `core.event` ClosureMethod row and
`core.event.__event_with_policy` to these exports. A handler is the moved
closure box with the typed `lazy.callback` adapter (the payload's native slot
is lent to each handler, which reads it) and `lazy.callback.drop` as its
release; a consumed owned payload is released after the dispatch. Sema
(`Calls/EventHandles.jet`) produces `THandleOp.EventMethod` with the Rust
method table (`Collections.rs` event/subscription/event_scope/event_trace
method returns); handlers are retained `Fn(T) -> Unit` callbacks.
`JetRuntime/Tests/EventProviderWitness.c` links the real object with
`--wrap=jet_rt_alloc --wrap=jet_rt_free` and checks registration order,
priority, once, unsubscribe (also mid-dispatch), cancelled-scope registration,
scope and Event teardown, nested emit, the last Event handle dropped mid-emit
(freed blocks are poisoned), trace text, and that every scenario
returns to its allocation baseline. `Examples/features/concurrency/event_lifecycle.jet`
is the end-to-end golden. AsyncEvent, Hook and DecisionHook are not part of
this provider.

### Native Process value enums

`JetRuntime/Source/ProcessEnums.jet` supplies the complete ResourceLimit6 and
StreamMode3 families. They reuse `EncodingErrors.jet`'s eight-byte enum owner
helpers, not a second ownership implementation. Native tags are declaration
order in `DataABI.jet`: WallTime/CpuTime/Memory/OpenFiles/Output and
Stream/Inherit/Capture. These private native handles do not describe Rust boxes
or establish source/MIR admission.

ResourceLimit show/display use `wall time`, `CPU time`, `memory`,
`open file handles`, `output`; Debug uses the variant names. StreamMode has
no renderer or constructor export. The real-object C client
`Tests/ProcessEnumsWitness.c` checks every legal tag, cross-tag inequality,
clone independence, all three ResourceLimit renders and exact-once frees
with `--wrap=jet_rt_free`. Its `.stdout` file is expected output, not an
execution receipt. Whole-unit seed suppression and native caller/import/layout
admission remain composition gates.

`ServiceEnums.jet` uses those same enum-owner helpers for the private
JetServiceRestart and JetServiceDelivery handles. Declaration-order tags
from `Services.rs:39-44,92-96` render exactly `OneForOne`, `OneForAll`,
`RestForOne` and `AtMostOnce`, `DurableAtLeastOnce`; show and display agree.
These are native providers, not Core source-module registrations or Rust
enum layout aliases. `ServiceEnumsWitness.c` checks every tag, pairwise
equality, independent borrowed-input cloning, independent rendered owners,
null drop and exact-once shell frees against real providers with
`--wrap=jet_rt_free`. Its stdout fixture is an expectation, not an execution
receipt. Native layout/MIR/import admission, seed-body suppression and actual
link/execution remain the same composition gates.

`ServiceError.jet` owns a private 16-byte tag/Text-pointer carrier in
`DataABI.jet`, with declaration-order tags from `ServiceAuthority.rs:198-216`.
Its ten constructors clone borrowed Text; clone is deep, equality compares
the variant before its payload, and drop releases both owners exactly once.
Each getter checks its variant before cloning Text, using the shared
`rt_record_variant_panic` on mismatch just as `CAbiRoutes.rs:11197-11303` does.
Show returns a fresh copy of the raw payload, including NUL and Unicode:
`Services.rs:1248-1263` matches all ten variants to `m.clone()`, with no
label, escaping or redaction. No display/debug API is added by analogy.
The real-provider `ServiceErrorWitness.c` checks all variants, ownership,
byte-exact show/getters and inequality; `wrong-N` modes test each getter's
variant failure against the existing record-variant stop contract. Its
stdout fixture is expected output only; native execution and source-layout
admission require composition, not a syntax receipt.

`ServiceValues.jet` implements the plain StateStore and Runtime carriers,
not any channel, authority or task owner. Their private layouts in `DataABI`
are an owned Text pointer and an owned Text pointer plus raw i64 retention.
Constructors borrow inputs, clone Text and check exact Int narrowing before
allocation; getters return fresh owners. Retention remains unchanged,
including negative values. Runtime has no equality export. Show/display use
the exact `Services.rs:1142-1150,1187-1195` formats and append the existing
signed decimal formatter directly without allocating a temporary Text.
The Runtime constructor follows `CAbiRoutes.rs:11319-11320`'s `native(m1)`:
`CAbi.rs:169-172` stops on out-of-range exact Int. The native
`jet_int_owned_to_i64` (`Int.jet:370-373`) selects `int_stop` reason 4,
which delegates to `trap_native_index_range_stop` (`Int.jet:337-340`).
`ServiceValuesWitness.c` tests both signed endpoints, negative and zero
retention, NUL/Unicode Text, ownership and canonical narrowing stop modes
against actual providers. Its stdout is an expectation; execution, native
admission and old-body suppression remain composition checks.

`ConditionHandles.jet` provides named handle clone/drop on the existing
runtime-native Condition48 returned by `jet_rt_native_condition_new`.
Cloning retains the same protocol, and dropping the last reference frees it;
it never copies FIFO waiters, pending notifications, epoch or mutex state.
This matches `MathTaskMem.rs:3081-3099`'s shared protocol semantics without
reinterpreting its Rust Arc/Box representation. The named null drop is inert.
`ConditionHandlesWitness.c` uses the real native factory and notify kernels
to observe clone identity, shared pending notifications after partial drops
and exact-once final release under `--wrap=jet_rt_free`. Its stdout fixture
is an expectation, not execution evidence. Named CAbi nominal route
admission and seed-route regeneration must happen in the composed native
unit; these providers do not introduce a second factory or protocol.

`CountMinSketch.jet` is the native Count-Min producer and mutable owner.
CR4 card #4828 (`D-SKETCH-CLONE1`) requires the seed's Arc/Mutex sharing:
handle clones retain the same 4112-byte state, not independent row snapshots.
The state contains reference count, mutex and four rows of 256 packed u32
counters. `Sketch.rs:3-18,125-168` supplies the two wrapping FNV hashes,
declaration-order row indexing, saturating increment and minimum count.
Hashing is outside the lock; all four counter reads or mutations are inside
one unchanged mutex scope. Count returns a fresh exact Int; show is exactly
`CountMinSketch` (`Core.rs:666-670`) and does not read rows.
The actual producer `jet_count_min_sketch_new` and canonical
`jet_rt_native_cms_add`/`jet_rt_native_cms_count` exports serve both runtime
method callers and `CountMinSketchWitness.c`, without test-only adapters.
The witness checks deterministic rows, clone-visible mutation, u32 saturation,
concurrent shared increments and last-owner release with `--wrap=jet_rt_free`.
Its stdout is expected output only. Native method-rail admission and
seed-route regeneration remain composed native-backend gates.

`HyperLogLog.jet` uses the same shared ownership rule on Native272: references
at 0, mutex at 8 and 256 packed U8 registers at 16 (stride exactly one).
Its real producer and canonical `jet_rt_native_hll_add`/`jet_rt_native_hll_count`
operate on this state; clone retains the same pointer. FNV hashing is outside
the single mutation lock. The logical hash shift preserves the seed's eight
high zero bits, including its rest-zero rank 57. Count scans in register order
under one lock, then unlocks before exact-Int allocation or show rendering.
The no-empty estimator preserves `Sketch.rs:55-57`'s sequential sum and alpha
operation order, with exact powers of two from audited float bitcasts and
positive round-half-away followed by saturating i64 conversion.
For empty-register counts 1..256, `hll_linear` freezes the actual seed's finite
domain instead of introducing a logarithm provider. The one offline oracle
includes the actual `Sketch.rs`, SHA256
`7d96fe7210b904876ac89a87a319f3cced6bf1758f937ef795262d83eb8851be`;
its frozen output SHA256 is
`b8c28826e186fb5c2acd43f821afaae7588893e09c3417b94a892d0c1f962fc7`.
No entry is within 1e-9 of a half-integer; nearest is empty140, distance
0.004965598786071723. Cargo is only an offline seed oracle, never a provider.
`HyperLogLogWitness.c` checks all finite-domain counts, frozen full-register
vectors, uniform ranks through 255, mixed ranks, exact show, shared concurrent
mutation and last-owner free. Its stdout file is expected output, not proof;
native method routing, execution and obsolete seed suppression remain gates.

`TDigest.jet` preserves `Sketch.rs:61-122` on a shared Native40 owner
(references0, mutex8, embedded canonical sequence24 at16). Its centroid buffer
has stride16, mean0 and weight8. One mutex covers insertion/compression; one
covers quantile. The exact rustc1.97.1 `slice::partition_point` binary-search
shape is retained, including its non-monotone NaN behavior. In-place
compression is safe because write never overtakes read, and avoids the seed's
temporary allocation without changing floating operation order. In particular,
cumulative advances only when a centroid is pushed, not when it is merged.
The canonical new/add/quantile exports serve real callers and the C witness,
which compares all 250 frozen centroid pairs in `TDigestVectors.txt` bit for
bit after 512 samples and reports the first mismatch index. Pass that vector
file as argv1. It also checks ten seed quantiles, NaN at first/middle/last,
IEEE edges, concurrent shared mutation and final buffer/shell ownership.
The separately authorized NaN-only offline seed output SHA256 is
`f26a9917f71a55a4e476d2e32554fc4680d1f247116e787c3161367b98bedbb9`.
This is reference data, not a shipping Rust dependency or native-execution
proof. Composed native routing, execution and seed suppression remain gates.

`ReservoirSampler.jet` retains the seed's shared Native64 state:
references0, mutex8, clamped raw-i64 limit16, unsigned count24, RNG32 and
embedded canonical List24 at40. The list's owned E4 Text elements have
stride8; sample returns a separately owned holder and deep-cloned elements
using the existing ProcessSpec list helper and the native List layout from
`Lower/Lists.jet:1-7`. Lower consumers own the returned inline header; a C
client releases its allocated holder after dropping elements and buffer.
The seed xorshift13/7/17 starts at deadbeefcafebabe, advances only after the
reservoir fills and uses unsigned modulo over the full count. Do not replace
it with Core's divergent SplitMix64 ordinary record. Storage grows lazily,
even when the configured limit is I64.MAX. Each operation holds one mutex;
show snapshots the count and unlocks before rendering its full U64 decimal.
The U64.MAX increment invokes the existing canonical integer-overflow panic
before mutating the state, rather than creating a new trap or wrapping to zero.
The real-producer witness checks every frozen slot and RNG state after
128 samples at five clamped capacities, clone sharing, sample independence,
raw NUL/UTF8, concurrent adds, both unsigned endpoints and exact-once
Text/data/buffer/shell release. Its overflow mode expects the canonical
overflow stop; stdout expectations are not execution evidence. Canonical
method routing, execution and obsolete seed suppression remain composed gates.

`EmailError.jet` provides the three retained Error clone/drop/equality exports
on the registry-plain `EmailError` layout, not on a Rust enum or private facade.
`Lower/Lower.jet:439,443` and `Lower/Values.jet:49-55,65-74,111-127,137-145`
give a 40-byte enum box: tag at 0; owned operation Text at 8; nullable server
Option-box pointer at 16; nullable code Option-box pointer at 24; owned reason
Text at 32. Both Option boxes are eight bytes, with their payload at zero.
Clone deep-copies Text and Option boxes and retains the exact Int code without
narrowing; drop releases owned contents before their boxes. Equality checks tag
first, then operation, server, code and reason in declaration order. Producers
are checked source literals and native SMTP, so no constructor facade is exported.
All three operations validate the tag in 0..10 through the shared record-variant
stop. Option allocation (`ProcessSpec.jet:4-7`) and release
(`EncodingErrors.jet:16-17`) use the identical `ENCODING_ENUM_BYTES`/alignment-8
payload-box contract. Witness modes `bad-clone`, `bad-drop`, `bad-eq-left`,
`bad-eq-right` (also with `-negative`) require the canonical record-variant stop,
exit 70 and empty stdout; they cover invalid tags 11 and -1.
`EmailErrorWitness.c` uses real allocator/Text/Int providers to build that native
layout and checks all eleven tags, nullable fields, raw text, arbitrary-precision
codes, clone independence and exactly-once release. Its stdout fixture is an
expectation, not an execution receipt. Post-freeze native cutover must retire the
opaque `EmailError`→`Error` rows in `RuntimeRoutes.jet`, admit the canonical plain
enum at the Core-definition override and permit real source literals in
`Places.jet`; native execution and obsolete seed-export suppression remain gates.

### Native UI scalar and record carriers

`UiCarriersABI.jet` in JetBackend/Source defines opaque native-private
TerminalSize, TerminalPolicy, TextWidth and Browser metadata carriers.
It is not the layout of a Rust allocation or a checked-MIR source record.
The Lower-facing Node/Accessibility/Shortcut contract remains the shared
`UiABI.jet` precedent; these scalar carriers do not introduce another Node
layout or closure convention.

Rust `i64` fields (TerminalSize cols/rows, BrowserTimeout milliseconds and
BrowserLocked size) remain raw signed words. Constructors checked-narrow
exact Int inputs; getters return fresh exact Int owners. Clone/equality/drop
never retain or destroy an Int owner for those stored words. Browser text
fields own independent E4 headers and buffers, and text getters clone them.
Record enum fields are raw declaration-order tags; unit handle traits own
separate 8-byte carriers.

TerminalPolicy24 embeds the byte-identical TerminalSize16 at offset zero.
Its constructor copies the two raw size words and frees the consumed Size
via its canonical drop exactly once, before checking the mode as the pinned
constructor does. Policy clone allocates only its own 24-byte carrier; the
Size getter returns a fresh 16-byte owner. Equality compares cols, rows and
mode, with no pointer-identity shortcut.

`UiCarriersBasicWitness.c` targets the real Jet providers with allocation/free
wrapping: clone independence, exact-once release, signed extrema, enum
ordinals, embedded NUL/UTF-8 text and distinct record fields. Separate
`width-stop` and `policy-stop` process modes expect the existing E3010 footer
and exit 70. Syntax/ABI receipts do not claim linking, process execution,
checked caller layout or native-only admission.

### Native civil-time ownership boundary

`TimeGlueABI.jet` defines private Date24, Period24, LocalTime32, Duration8
and DurationUnit8 carriers. It does not define Core Instant: that source
value owns an exact Int field by value, as the Realtime deadline route
already returns (`Realtime.jet:144-155`). Instant carrier admission belongs
to the generator, not a Rust-pointer interpretation.

Date/Period/Duration record constructors borrow and check exact Int inputs
before allocation, store raw i64 and return fresh exact owners on get
(`CAbiRoutes.rs:9382-9390,10174-10194,10982-11002`). These record constructors
do not add civil-calendar validation. Clone/drop own only the shell.
Equality follows declaration order (`Time.rs:4-9,303-309,484-489`,
`CommonTypes.rs:2840-2843,2866-2874`). LocalTime nanos is u32, not signed
i64; clone clears native upper-word padding and equality ignores it.
There is no LocalTime record ctor/getter in this HAND family.

All modes preserve the seed formats (`Time.rs:280-286,472-480,619-620`,
`Duration.rs:195-197`), using existing decimal digit counting/emission and
the existing text builder. Date years have minimum four digits and a
separate negative sign; the seed's saturating absolute value makes i64::MIN
render with magnitude i64::MAX (`Time.rs:1760-1765`). LocalTime emits
unsigned nanos only when nonzero, padded to at least nine digits.
Periods retain signed components without normalization; durations append
`ns` to signed nanoseconds.

`TimeGlueWitness.c` is a real-object client requiring `--wrap=jet_rt_free`
with asserted interception and exact-once frees. It exercises boxed Int
borrow/getter ownership, every equality field, nanos padding independence
and civil/duration render goldens at raw integer boundaries.
`date-overflow`, `period-overflow` and `duration-overflow` must stop at the
existing native-range boundary. Native execution and MIR/caller/layout
admission remain unexercised; parse/C11 syntax receipts are not parity proof.
### Native ProcessSpec value and pure producers

`ProcessSpec.jet` implements the six existing handle exports and the existing
pure C ABI builders over the single private Spec136 layout in `DataABI.jet`.
`jet_std_process_cmd` clones borrowed argv into the internal owned producer;
every `jet_process_spec_*` C builder borrows the parent and returns a deep
clone. It never mutates or consumes the caller's spec. Command and removed-name
lists own E4 text; environment pairs use ordered 16-byte tuples, preserving
duplicates. Nullable text/policy pointers own their nested carriers; nullable
raw-i64 boxes preserve absent versus present-zero for stdin and limits.

The defaults and transitions follow `ProcessSpec.rs:12-103` and
`Process.rs:56-101,170-187`. Exact Int limits clamp to 0..i64::MAX without a
native-conversion stop. Duration arguments remain borrowed handles using the
canonical `TimeGlueABI.jet` ns word and truncating division by 1,000,000 before
max(0). Raw stream ordinals are checked with the frozen C ABI's range stop,
not Int conversion. Terminal defaults are 80x24 Cooked and use the canonical
TerminalPolicy24 provider. Borrowed Bool field helpers return the address of
the live normalized byte and allocate nothing. Equality masks the low byte
of each Bool slot; clone normalizes it to a full-word 0/1, ignoring padding.

Show exposes only `ProcessSpec([cmd...])`, using the shared Rust `str` Debug
renderer (including NUL `\0`), not JSON or Jet string escaping. Equality still
includes every stored field, including policy wire and environment secrets.
`ProcessSpecWitness.c` uses real producers, verifies defaults, each builder,
borrow preservation, optional presence, ordered pairs, recursive independence,
all equality fields and omitted secrets, limit clamps above both i64 bounds,
Duration edges, all stream modes, exact render bytes and exact-once frees.
Separate stdin/stdout/stderr range modes require the native-index stop.
The expected `.stdout` is not an execution receipt. Native layout/import
admission and whole-unit seed suppression remain pack gates.

The plain execution providers in `ProcessExecution*.jet` reuse the existing
`HostSpawn.jet` Spec216/Child72 kernel seam. Preparation snapshots the logical
environment, validates argv/env/cwd, preserves held directory descriptors and
packs inherited/pipe/null streams and declared limits. The collector captures
both streams concurrently, kills and reaps attached process groups on timeout,
limit exhaustion or last-owner drop, and publishes one owned Receipt144.
Receipt metadata retains framed SHA256 input/policy digests, secret redaction,
exact Int status/signal/pid facts and the original typed errors. Native
ProcessReceipt/Plan field reads validate canonical identity and MIR member types
before lending the Jet-owned 144/72-byte carrier to typed read/clone glue.

Authority-bound execution stays fail-closed before spawn until the isolation
engine is ported; unbound `plan()` still requires `.under(...)`. Pipeline launch
also remains non-native: its shared budgets, direct-pipe versus bounded-relay
transaction, per-stage group cleanup and cancellation need a separate faithful
port. Its retained regressions include
`process_pipeline_streaming_lifecycle_matches_all_execution_tiers.case` and
`process_pipeline_timeout_reaps_descendants_across_execution_tiers.case`.
No serial or all-relay substitute is supplied.

`run-process-witnesses.mjs OUT RC-NATIVE-JET RC-RUNTIME-DIR` verifies the admitted
pack inventories and links fresh C clients to a held-byte snapshot of that
pack's standalone Jet object and admitted musl archives. A fresh output directory
is required; compiler, witness source, C object, executable, platform and CRT
identities are recorded and checked across compilation, linking and execution.
It covers real-child capture, Inherit, nonzero exit,
timeout kill, output limits, strict UTF8 errors, last-alias cleanup, controlled
World refusal, raw directory/env adapters and real PTY resize. It also runs the
native typed-field regression through the supplied RC Jet. Parse/tiny/C compile
receipts do not prove execution: native execution is pending the RC pack.

### Rust-free C ABI conformance clients

`JetRuntime/Tests/CAbiDriver.c` replaces the native test runner's Rust driver.
It calls only real runtime exports: `jet_rt_main`, text and terminal output,
Euclidean Int division/remainder, numeric stop and require stop. Missing
exports are link failures, not substitutes or skipped tests. The same client
has allocator-only process modes for comparison with a supplied seed oracle.

Run `node Compiler/JetRuntime/Tests/run-cabi-driver.mjs OUT OBJECT_OR_ARCHIVE...
[LINK_ARG...]` with the complete real native link group. The runner builds
with `CC` (default `cc`), not rustc or a Rust rlib, captures compiler exit/signal
and each process status and bytes, then checks all seven original modes:
values, text-carriers, euclid-by-zero, numeric-stop, require, require-eq, panic.
Values match `CAbiDriver.stdout`; normal stderr is empty. Stops retain the
original required diagnostic prefixes, locations, descriptor source/function
context, locals and Why heading; stdout must be empty and exit must be 70.
The eleven-word descriptor and mode behavior follow the seed driver
`tests/fixtures/runtime_c_abi_driver.rs:60-156`. Text24 coverage includes
embedded NUL/UTF-8, deep clone after input drop, empty sentinel, aliased
builder appends with and without growth, and same-header finish.

Run `node Compiler/JetRuntime/Tests/run-alloc-parity.mjs OUT SEED_LINK_INPUT...
--native NATIVE_LINK_INPUT...`. It builds the same C client with
`C_ABI_ALLOC_ONLY`, once for each explicitly separate real link group. The
native group must contain no seed Rust artifact. Invalid layout requires the
original panic prefix and exit 70; exhaustion requires SIGABRT and exact
`memory allocation of 9223372036854775800 bytes failed\n`. Both modes compare
the complete seed/native stderr, with empty stdout and no spawn error.
The seed group is an external comparison oracle, not a shipping dependency.

These runners no longer consume `runtime_c_abi_driver.rs`. The Rust reference
integration test's fixture remains seed-only until that test tier is removed;
TestLead1 retired its native contract from the Jet test kit and assigned
this successor to the runtime line. C11/Node syntax checks do not establish
native linking, runtime execution or stop-byte parity; those require a
composed real runtime link group and actual runner receipts.

### Sentry fault-detection ledger parity

The shared checker observes a nonzero fault immediately after classification
releases its lock, before returning the fault tag. This matches
`MemSentry.rs:607-617,704-717`; a successful or disabled check writes nothing.
The stop/report path no longer records the same fault a second time
(`Mem.rs:78-88` and `Core.rs:1482-1517` likewise only report after detection).
The existing gate/release/hardened/fence rules and observational ledger writer
are unchanged. `JET_MEMORY_LEDGER` selects a nonempty explicit path; otherwise
the normal default remains `.jet/memory/ledger-v1.jsonl`.

The world/sentry runner supplies fresh, per-run ledger paths for every raw
check fixture so tests never write the checkout's default ledger. Its return
and stop modes both use an actual expired-frame pointer classification,
require exactly one JSONL row, and compare their complete bytes with each
other and the existing canonical reference writer. Syntax receipts do not
establish executed fault-detection parity.

### Source-gate memory ledger test port

`SentryTests.jet::jet_rt_sentry_fixture_source_gate` ports
`tests/unsafe_sentries.rs:3-28` using the real native scope and checker.
It enables the `unsafe.jet:12` gate with reason `reads raw storage`, checks
address 1 for eight bytes/alignment 1 under strict provenance, and requires
R0801 with the source metadata retained before scope leave/reset.

`node Compiler/JetRuntime/Tests/run-sentry-source-gate.mjs OUT REAL_FIXTURE`
runs the composed `world-sentry-fixture` client's `source-gate` mode with a
fresh isolated ledger path. It requires exit 0 and empty stdout/stderr, then
the original six ledger substring predicates: schema, kind sentry, code
R0801, source unsafe.jet, source-Unsafe-gate provenance, and the derive-from-live-
storage repair. It additionally requires exactly one row to guard the
fault-detection/stop separation. The client must be composed with the actual
SentryTests source; no Rust driver or fabricated ledger writer is used.
Native execution is not established by the syntax receipt.
This Node runner is development-test debt: at native cutover, replace
its launch and all six ledger predicates with the Jet test kit runner, then
remove the Node implementation after identical exercised results.

### Native data metadata mirrors

`DataMetadata.jet` supplies the 36 native-mapped exports for DataFormat,
DataLoaderKind, DataAuthority, DataSourceIdentity, DataProvenance and
DataSnapshotIdentity. These mirror `Core/data/data.jet:29,164-187`, not registered
source-owned nominals. A future source-module registration must retire the
corresponding hand carrier and seed glue in one composition.

`DataABI.jet` defines the private carriers: shared enum8, Authority16,
SourceIdentity32, Provenance24, Snapshot40. Nested text/list/record pointers own
deep independent values. Equality follows Rust field order. Record text getters
return fresh owners, source text field helpers copy read/clone-only 24-byte stack
headers, and provenance field helpers lend its nested records. Existing Authority
and Snapshot constructors clone borrowed texts; snapshot format is checked before
allocation. No SourceIdentity or Provenance constructor export is invented; real
native producers and the ABI witness construct these carriers.

`DataPublicText.jet` is the single private rendering policy port from
`PreludeDataFlow.rs:167-230,265-307,738-739`: Unicode whitespace trim, ASCII-only
case folding, first `=` split, SHA-256 digest for empty/keyless parameters,
substring-sensitive keys, prefix-sensitive values, raw member bounding at a
4096-byte UTF-8 boundary, fragment removal and first userinfo `@` removal after
`://`. Locator base text is not bounded. Query entries preserve empty segments,
order and duplicates. Source list parameters are rendered separately with `?`,
even when the locator already has a query. Authority bounds/redacts each part;
Snapshot text remains unbounded and unredacted. Shared `Sha256.jet` owns the hash
algorithm; the public renderer borrows input bytes and frees its one scratch
block after appending lowercase hexadecimal.

`Tests/DataPublicGoldens.h` was frozen by the actual Rust Foundation functions,
not a reimplementation: rustc 1.97.1 (8bab26f4f 2026-07-14), oracle tool exit 0,
5.14 seconds. Generator command (one-time development oracle, light lane):
`Tools/agent/jet-env rustc --edition=2021 ~/.cache/jet-dev/scratch/RtBurn1b/data-public-oracle.rs -o ~/.cache/jet-dev/scratch/RtBurn1b/data-public-oracle`,
then that binary with `Compiler/JetRuntime/Tests/DataPublicGoldens.h`.
Both invocations ran through `CHEAP_LANE=light ~/.cache/jet-dev/cheap.sh`.
SHA-256 receipts:

- fixture: `b084a2de95e5a4154f4c016ccbff708b01a4f89a39884baa0be418df49f99f4a`
- Foundation policy: `badd03a1f45f2c80a844245c1bb7459207b2a28ec4c42154bccb2e02831ebbd7`
- Foundation SHA: `45f0dd77827f9d6dfd5eb8ae86571b8f2807b5419a30f4270092c7e1682a6e9b`
- generator: `b18c9b1d2f152e9bed86fb8da54dfbe1bc2b61c8ba63078f413da6c3aab5f5bb`

`DataMetadataWitness.c` links only actual native providers and frozen vectors,
with `--wrap=jet_rt_free` for exact-once deep-owner release. It covers all 36
families, constructor borrows, fresh getters, field lifetimes, every equality
field, enum labels, redaction, first-`=` handling, SHA digests including empty
input, URL/userinfo/query boundaries, Unicode trimming and multibyte truncation.
Its `negative` and `range` modes require the canonical native-index-range stop,
exit 70 and empty stdout. The stdout fixture is expected output, not a native
execution receipt. Composition must admit fresh layout/MIR/imports and suppress
obsolete seed exports before linking/running; syntax checks do not establish
runtime parity or Rust/toolchain retirement.

### Raw file and process host boundary

`JetRuntime/Source/HostFiles.jet` owns the Linux descriptor transport. Its
canonical FD72 header has the futex mutex at 0, descriptor at 8, closed flag
at 16, owned E4 path at 24, buffer at 32, capacity at 40 (negative for
writes, zero for raw readers), filled bytes at 48, position at 56, and refs at 64.
Explicit close affects aliases; last release frees the state. Portable stat
facts hide the kernel's stat layout, and statx reports birth-time availability
instead of assuming it is absent. Held-directory operations never reopen a
source inode or substitute an ambient pathname.

Core owns validation, fault injection and IOError construction. Private leaves
report ErrorOut32 (domain/stage U32, code I64, kind/pad U32, owned E4 message)
and a separate WriteZero fact on write/flush/seek/close. Linux syscall errors
have POSIX domain 1 and zero kind/message. Read and descriptor dup first flush
pending writer bytes; their flush WriteZero error uses custom domain 4, Other
kind and the owned canonical cause because those ABIs have no Bool output.
Reads then make a single kernel read into caller-owned compact bytes, never
consume the write buffer, and preserve Interrupted. A dup owns a fresh empty
buffer and sees the flushed shared kernel offset. Historical
`958047768^:crates/jet-codegen/src/Prelude/CoreLib/JetStd/FileHandles.rs`
owns unbuffered `std::fs::File` descriptors, so private native buffering must
not hide preceding writes from either operation. Its `jet_fs_link_fd_at_raw`
calls AT_EMPTY_PATH once with no pathname or `/proc/self/fd` fallback.
Linux [link(2)](https://man7.org/linux/man-pages/man2/link.2.html) requires
CAP_DAC_READ_SEARCH for that flag and reports ENOENT without it; the transport
returns that fact unchanged.

Explicit `jet_std_process_exit` keeps the canonical exact-Int-to-signed-i32
conversion and calls `jet_rt_process_exit`; it no longer independently flushes
stdout and invokes `exit_group`. The common runtime process boundary owns the
process-wide winner claim, atexit handling and final output flush. This caller
requires the separately linked Jet Main provider of `jet_rt_process_exit`.

The six-argument process launch
borrows ExecCandidates32 (count, NUL candidate column, script-fallback bit,
searched bit) and Core-prepared argv and effective environment bytes. This
preserves non-Unicode PATH entries without an independent environment. Source
computes the exact reference route's PATH and ENOEXEC policy. The runtime parent
prepares allocations, descriptor copies, limits and `/bin/sh` argv before fork.
The child uses only raw syscalls: stdio mapping, cwd, source-prepared group
setup, SIGPIPE reset, held-directory mapping/chdir, limits/session/PTY setup,
then ordered execve. GNU retry facts remember EACCES, continue past ENOENT,
ESTALE, ENOTDIR, ENODEV and ETIMEDOUT, and stop on all other errors; an explicit
fallback bit alone permits the preallocated `/bin/sh` argv. Child72 has its mutex at 0,
pid/reap-status at 8/16/24, stdin/stdout/stderr/PTY at 32/40/48/56, and refs at
64. Taking a stream transfers an independently owned FD72; terminal input and
output duplicate the master while taking terminal control transfers the
original. Stream aliases may outlive the child. Windows-only PTY control
carriers use capacity I64.MIN and dispatch resize/close to the OS console
handle; Linux and macOS never produce that tag. Native carrier fixtures link
against the complete composed runtime object, not an allocator-only seed.
`realpath` into a Jet-owned fixed 4096-byte buffer preserves exact OS symlink,
`..`, and non-Unicode canonicalization, with the real OS errno on failure;
this ledgered mechanical import is not a pathname-policy replacement.

Owner directive #33 defines an empty exec-candidate set as ENOENT, without an
execve attempt. This is the intentional I9 delta from glibc's oversized-PATH
case that otherwise returns whichever stale errno happened to be present.
Child setup retains its reference error precedence: an invalid cwd or descriptor
still fails at its own stage before the empty candidate set reports exec ENOENT.

The remaining held-file calls reuse this boundary, without exporting a second
set of public-policy adapters:

| Core seed route | Jet raw transport |
| --- | --- |
| `jet_std_files_open_writer` | `jet_fd_open_raw` / `jet_fd_open_at_raw` |
| `jet_fs_directory_close` | `jet_fd_close_raw` |
| `jet_fs_directory_create` | `jet_fd_mkdir_at_raw` |
| `jet_fs_rename_at` | `jet_fd_rename_at_raw` |
| `jet_fs_read_link_at` | `jet_fd_readlink_at_raw` |
| `jet_std_file_reader_stat` | `jet_fd_stat_raw` |
| `jet_std_file_writer_set_mode` | `jet_fd_chmod_raw` |
| `jet_fs_directory_fd_raw` | `jet_fd_number_raw` |
| `jet_fs_stat_at` | `jet_fd_stat_at_raw` |
| `jet_fs_xattr_get` | `jet_fs_xattr_get_raw` |

`jet_fd_stat_at_raw(directory, name, StatFacts128*, ErrorOut32*)` follows
FileHandles.rs:711–758: an empty name stats the held directory; a final entry
opens with `O_PATH|O_NOFOLLOW|O_CLOEXEC`, stats the owned descriptor, and closes
it, including stat failure. The source resolves/validates intermediate names.
Ambient path stat continues to use `jet_fs_stat_raw`, not a fake directory.
`jet_fs_xattr_get_raw(path, attribute, compact_bytes, length, ErrorOut32*)`
performs one `lgetxattr` syscall, returning the byte count or -1. A null buffer
with zero length queries size. FileHandles.rs:988–1031's absent/unsupported
mapping, allocation and ERANGE retry remain source policy. Descriptor numbers
are borrowed facts: source retains the directory until the consuming leaf
duplicates it; they are not independent owned handles.

### Remaining native String routes

`TextRoutes.jet` supplies the C route ABI for chars, copy, byte count, lines,
repeat, replace, both explicit slice forms and ASCII case conversion, reusing
E4 ownership, UTF-8 scalar decoding, constant-storage substring search and the
existing character-range failure report. Chars/lines return an owned word
column plus its count, with null for empty columns, matching generated
`list_out`. Byte count returns the canonical exact-Int word. Lines match
Rust `str::lines`: remove LF and its optional preceding CR, preserve bare CR,
and omit the extra line after a final LF. Replace uses nonoverlapping matches;
an empty pattern inserts at each Unicode-scalar boundary including both ends.
Repeat clamps negative counts to zero and copies into one exact allocation.
ASCII casing leaves every non-ASCII byte unchanged. Slice bounds are character
indices and exclude the end, including an empty slice at the character count.
The generated route exporter removes replaced Rust definitions while retaining
shared carrier metadata; no second String carrier is introduced.

The x64 runtime-pack census excludes wasm32-only allocation exports (WebAbi,
font/HarfBuzz scratch, numeric Web buffers and Web D-FRED). Those U32 linear-
memory/provenance/TLS contracts remain separate target rows, not native-pointer
casts. Date/time formatting and Pool construction are Core source-owner rows,
not String/allocator runtime aliases. The private image-runtime stop/String
allocator and L1/L3 canonical allocator/split providers are existing Jet bodies.

The inherited 42-row disposition ledger retains every row: ten String route
ports and four entry/Err ports are source-complete with light receipts only;
four image/allocator rows and two canonical split replacements already have
Jet bodies; five time-format rows belong to the Core time owner, Pool to Core
Mem, the comptime-stop writer to its runtime owner, and fifteen wasm allocation
rows remain separate target contracts. `jet_err_from_message` is Err88 source
`453a68026`; the three entry exits are `JourneyTLS.jet` source `695b6dc40`.
No row is discarded or promoted to native behavioral proof by a parse receipt.

### Native shared process pipe holder

`HostPipe.jet` owns `Pipe24`: non-atomic reference count at 0, RefCell mutable
borrow state at 8, and an owned reader FD72 pointer at 16 (`0` means `None`).
The source facts are `CommonTypes.rs:162–165`'s
`Rc<RefCell<Option<BufReader<ProcessReader>>>>` and `Process.rs:3325–3343`'s
shared mutable buffered position. Clone returns the same holder and increments
its count; only the last drop releases the FD72 and frees the holder. The FD
is taken from `jet_process_take_stream_raw`, not borrowed from or retained
through Child. Shared close takes the optional reader, so every clone sees EOF.
No Rust Box/Rc/RefCell offset and no independently duplicated fd is involved.

The four `jet_rt_handle_{clone,drop}_process_{stdout,stderr}_stream` exports use
the canonical handle symbol convention. `jet_process_stream_line_bytes_raw`
borrows the holder and returns `Result<[U8], IOError>` as a native tag plus
owned compact data/length or an owned error. It shares FD72's refill buffer,
includes LF, preserves malformed UTF-8 and bare CR, and retries Interrupted
as `BufRead::read_until` does. Checked Core `__stdout_next_line` and
`__stderr_next_line` own empty-byte EOF, UTF-8 decoding and the one-LF/one-CR
terminator policy. No fixed host next-line iterator substitutes for them.

When TAKE supplies a writer-mode FD72, the first buffered read flushes pending
output under the FD lock before converting its buffer to reader refill mode
and resetting both cursors. Flush failures leave the writer suffix intact and
return through the fallible read edge (TAKE itself has no error result).
Writes through retained reader-mode aliases bypass refill storage and use a
kernel write, so they cannot corrupt unread lines and a pipe read-end write
returns EBADF.

A conflicting mutable borrow panics with `RefCell already borrowed`, matching
the seed's observed rustc 1.97.1 commit `8bab26f4f` and that commit's
`library/core/src/cell.rs:902,912–917`. Its trap path does not allocate.
`Tests/process-pipe.c` plants real-descriptor, shared-refill, interleaved-clone,
shared-close, actual comptime-stop release and signal-reentrant borrow
witnesses. Execution and allocation-ledger-zero remain unexercised; the latter
requires the ProgramAllocator counting/facts provider, not a fake allocator.

### Native default error carrier

`JetBackend/Source/ErrLayout.jet` defines the native-pack Err88 ABI. It is not
the legacy Rust seed layout or the source-visible three-field MIR record:
identity, context, conversions and typed details remain owned by the opaque
native handle. `JetRuntime/Source/Err.jet` clones every child independently;
the C record constructor clones borrowed message/code but takes the cause,
matching `CAbi.rs:372–377`. Getters return owned projections. Display/show/debug
return message only (`Values.rs:317–330`); `ErrReport.jet` separately ports
Outcome's recursive root report and ordered `jet.err/v1` serialization,
including Unicode C1 control escapes and raw canonical typed field JSON.
`Tests/err-report.c` exercises the real carrier and allocator LIVE BYTES fact
(index 2), not historical allocation count, for a deep-clone/drop ledger.
The native-pack source batch depends on G1f's `135db4321` generated-adapter
guard: `exports.list` selects each same-name Rust adapter's
`cfg(not(jet_native_pack))`. Hand-written `CAbi.rs` Err constructors/getters
carry the same guard explicitly, so the seed tier remains unchanged while
native packs contain only the canonical Jet implementation.
`run-host-fixtures.mjs audit` requires exactly one strong pack definition for
every Err/report export rather than accepting a mixed Rust fallback.
`JourneyReport.jet` ports Outcome's scalar-column terminal policy without
intermediate rendered measurement strings. Its goldens reuse literal expected
bytes from `Outcome.rs:2066–2172`, covering stream/80-column/40-column,
independent ANSI coloring, path-segment elision, note truncation and repeated
hop counts.
`Journey.jet` owns the pure Journey32 accumulator over an explicit caller slot:
new-origin resets old handled state, only consecutive equal function/file/line
sites collapse, the first site's column/note remain, clone copies all children,
take drains once, and adopt transfers carried children. The real
`jet_rt_journey_destructor(header,value)` has the Closure24 callback signature.
`JourneyTLS.jet` uses one dynamic key registered after thread initialization
with that real Closure24 destructor. E2 FINAL `d52672e62` supplies the reviewed
checked named-FunctionAddress mechanism; `7c69104b7` adds its Journey row and
`05bc3fac3` tests missing-export/wrong-arity refusal and the actual target.
Origin/frame/reset routes and report/entry exports join this same source unit.
There are no Journey-private TCB offsets. Taking a journey clears the dynamic
slot once; adopting a carried origin discards handled state.

The report edge resolves live process wire mode and stderr facts once:
NO_COLOR presence overrides FORCE_COLOR presence (including non-UTF8 values),
columns are positive trimmed COLUMNS on a terminal only, otherwise 80 for a
terminal or no budget for a stream. Root leads trail, trailing root newlines
collapse to one, and typed JSON retains the original Err88 metadata. Every
entry-error exit writes that report and uses the shared `jet_rt_process_exit`
boundary with status 1. `journey-tls.c` witnesses real thread isolation,
registered teardown, take/adopt/reset, ledger balance, and process status/bytes.
N=1000 owned getter/drop cycles also preserve Err88 LIVE BYTES; Lower's separate
borrowed hand-MIR witness requires no getter, clone, or allocation Call.
These behavioral/ledger/object witnesses require the composed proof; light
parse/C-syntax receipts alone do not establish them.
Presence is read allocation-free under the logical environment cell's Shared
reader lock; COLUMNS uses that cell's strict Unicode getter. Runtime set/remove
mutations therefore affect subsequent reports rather than freezing startup
environment state. The fixture toggles NO_COLOR after initialization and tests
FORCE_COLOR, invalid-UTF8 presence, changed COLUMNS and unsigned-width overflow.
Initialization order is explicit: allocator priority 0, Host priority 1, Thread
priority 2 (registry publication and main attach), then Journey priority 3.

`LogicalEnvironment.jet` replaces the executable-local Rust environment cell
with one owned native reader/writer cell using the existing Shared protocol.
Host argument registration seeds it from owned startup bytes before publishing
argc; reads and writes never mutate libc's environment or the immutable startup
index. Unix byte equality, startup duplicate replacement, and remove-then-append
updates follow `EnvInit.rs:6-35` and `FSIoEnvOsTesting.rs:688-715`.
The strict getter deliberately collapses absent and non-Unicode to zero because
the reference `jet_std_env_get` signature is `Option<String>` (569-570).
The OS getter preserves arbitrary startup bytes and returns presence separately:
absent is `(0,0,0)`, present empty `(1,0,0)`, nonempty `(1,owned data,length)`;
callers free nonempty data with exactly length bytes and alignment 1.
Set returns 0 / -1 InvalidName / -2 InvalidValue; unset returns 0 absent /
1 removed / -1 InvalidName. These are raw leaf statuses, not guessed enum tags.

CFG reachability pruning removes orphaned place metadata as well as dead value
facts: an index, range, lookup, or temporary base defined only in an eliminated
block cannot remain in an unused place row. Dead local aliases (including loop
bindings backed by an index) disappear with those rows; actual storage cells,
surviving operations, and drop edges keep their places. Real dangling references
still fail legality. Rust and Jet use the same rule; the dead-index regression
verifies MIR before/after optimization and executes both surviving branch paths.
