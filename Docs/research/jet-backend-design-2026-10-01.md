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

`LIR/Lint.jet` checks a module before any generator reads it: table IDs,
valid layouts, signature agreement of entry parameters, calls, returns and
jump arguments, single definition, dominance of every use, no use after a
`Move` or `Drop`, no `Copy` of a value with drop glue, Bool conditions, and
the shape of drop glue. `LIR/Print.jet` prints the text form used in tests.

## MIR -> LIR lowering

`lir_lower_program(program: MIRProgram) -> LIRLowering` lowers every MIR
function of a checked program into one `LIRModule`, appends the generated drop
glue, and runs the LIR lint on the result. `LIRLowering.issues` names the
function and the construct for everything the lowering cannot express yet, and
carries the lint findings; the module may only be handed to a code generator
when `issues` is empty. The lowering never decides meaning again (D-TIER-ONEIR1):
every decision below reads facts MIR already carries (checked types, ownership,
Prelude routes, phis, drop operations).

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
| `String` | runtime handle | `ptr` | `jet_rt_string_drop` |
| `RangeCursor`, `IterCursor` | runtime handle | `ptr` | `jet_loop_cursor_drop` |
| struct | heap box, field k at offset 8k | `ptr` | generated glue |
| enum, every variant without payload | discriminant word | `i64` | none |
| enum with a payload variant | heap box: discriminant at 0, payload slot k at 8(k+1) | `ptr` | generated glue |
| `T?` | null for absent, else one-slot box holding the value | `ptr` | generated glue (null-safe) |
| `Result<T, E>` | box: 1 (Ok) or 0 (Err) at 0, payload at 8 | `ptr` | generated glue |
| `Distinct`/`Alias` types | as their base | — | — |

Every box slot is one 8-byte word; a Bool field is stored in the low byte of
its slot. Boxes come from `jet_rt_alloc(size, align)` and return to
`jet_rt_free(ptr, size, align)`. A variant's discriminant is its declared
`discriminant` or its index. Narrow fixed-width integers, floats, lists, maps,
tuples, unions, trait objects, closures and generic instances are not lowered
yet and are reported.

A value's drop action comes from its MIR ownership fact: owned values
(`drop != NoDrop`, not a read or write borrow) carry the type's drop; borrows,
copies and every value of a type without drop carry none. MIR `Drop(value)`
becomes LIR `Drop` when the value carries a drop action and nothing otherwise.

Drop glue is one LIR function per box type, `drop.<canonical type key>`,
taking the box pointer and returning nothing (`LIRDrop.Function`). It loads and
drops every owned slot of the active shape (testing the discriminant for
enums and Results, and skipping null for Options), then frees the box. Glue
for nested boxes names further glue; the list grows while it is generated.

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
| `Constant` Int/Bool/Char | `Const` (an Int literal outside the inline range is reported) |
| `Constant` String | `DataAddress` of the literal's bytes, `Const` length, call `jet_rt_string_from_static(ptr, len) -> String` |
| `Copy` | the same value for word, flag and tag carriers; `jet_rt_string_clone` for a String; boxes need clone glue (reported) |
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
| `Struct` | `jet_rt_alloc`, one `Store` per field at its declaration slot |
| `Field`, field `ReadPlace` | `Load` at the field's slot |
| `Enum` | tag enum: `Const` discriminant; payload enum: box, `Store` discriminant and payload slots |
| `EnumIs` | `Compare Eq` of the discriminant (loaded from slot 0 for boxes) |
| `EnumPayload(index)` | `Load` slot `index + 1` |
| `Present(v)` / `Absent` | one-slot box holding v / null pointer `Const 0` |
| `OptionIsSome` / `OptionValue` | `Compare Ne` against null / `Load` slot 0 |
| `ResultOk` / `ResultErr` | two-slot box, slot 0 = 1 / 0, slot 1 = payload |
| `ResultIsOk` / `ResultValue` | `Load` slot 0 and `Compare Ne 0` / `Load` slot 1 |
| `BuildString(parts)` | `jet_rt_string_builder_new`, `..._push_static(b, ptr, len)` per literal, `..._push(b, text)` per Display interpolation, `..._finish(b) -> String` |
| `Semantic(Print(call, v))` | Display text of v (the String itself, or `jet_rt_int_to_string` / `jet_rt_i64_to_string` / `jet_rt_char_to_string` / `jet_rt_bool_to_string`, dropped after the call), then the route's symbol (`jet_term_write_stdout_line`) with `(text, flush = 1)` |
| `Call` of a user function, method or associated function | `Call Function(id)` with the callee's signature; Unit arguments are omitted |
| `Call` of a Prelude route | `Call Runtime(symbol)` |
| `LoopRangeInit/HasNext/Value/Advance` | the route's symbols; init takes `(start, end, step, has_step, exclusive)` |
| `Todo` / `Never` | the Todo route's symbol with the location / `jet_rt_unreachable(ptr, len)`, then `Unreachable` |
| `ScopeEnter`/`ScopeExit` | nothing for Unsafe, Impure, AssumeDeterministic, Layout and DebugOnly scopes; others are reported |
| `Drop(v)` | LIR `Drop` when v carries a drop action |
| `InitializeUninit` | nothing |

Every other operation (lists, maps, indexing, closures, trait objects, Core
calls, pattern captures, the remaining semantic operations) is reported as not
lowered yet, by name.

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
| `jet_rt_float_to_string` | `(Float in xmm0) -> String`, Jet Display of a Float |
| `fmod`, `pow` (C library) | `(Float, Float) -> Float`, Float `%` and `**` |
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
Linking: the runtime rlib plus a staticlib wrapper crate
(`extern crate jet_runtime;`, `--crate-type staticlib`) gives
`libjet_runtime_c.a`; link the object with it and `-lpthread -ldl -lm`.

### LIR additions for the lowering

The lowering added to LIR: `Div Rem And Or Xor Shl Shr` binary operators;
`Overflows(dst, op, l, r)` (Bool: signed Add/Sub/Mul overflow);
`Load(dst, base, offset)` and `Store(base, offset, value)` through a Pointer;
`DataAddress(dst, data)` with `LIRModule.data` (read-only bytes);
`LIRTerminator.Unreachable`; `LIRDrop.Function(id)` for generated glue; and
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
then `sub rsp, 8 * n` for spill slots, padded so rsp is 16-byte aligned at
every call. Slot k lives at `[rbp - 8 * (saved + k + 1)]`. Every return
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

- **Static executable** (`Image/ELF.jet`, `x64_static_executable`): one
  read+execute `PT_LOAD` segment at 0x400000 holding the headers, code and
  data; no writable segment, no section headers. `_start` calls the entry
  function and passes its word result (or 0) to `exit_group`. Runtime calls
  link against the freestanding image runtime below.
- **Relocatable object** (`Image/Object.jet`, `x64_relocatable_object`): an
  ELF64 `ET_REL` with `.text`, `.rodata`, `.rela.text`, `.symtab`, `.strtab`
  and an empty `.note.GNU-stack`. A global `main` calls
  `jet_rt_main(entry)`; every runtime call is an undefined global with an
  `R_X86_64_PLT32` relocation and every data address an `R_X86_64_PC32`
  relocation against `.rodata`, so `cc program.o libjet_runtime_c.a
  -lpthread -ldl -lm` binds the program to the one compiled runtime.
- **In-memory image** (`Image/Memory.jet`, `x64_memory_image`, for `jet run`
  and tiered execution): the linked block of the module's functions, one
  import stub per runtime symbol the code calls, and the data. A stub is
  `jmp qword [rip + 2]; int3; int3` followed by its 8-byte address slot
  (16 bytes, so every slot is 8-byte aligned); runtime calls are rel32 calls
  to the stub. The image records the entry offset and each import's symbol
  and slot offset. Loading (`Image/Loader.jet`, below) maps the block into an
  anonymous private mapping that is writable and not executable, copies it,
  writes the address of each runtime C-ABI export into its slot, flips the
  mapping with `mprotect` to read+execute, and runs
  `jet_rt_main(base + entry)`. No page is ever writable and executable at
  once; this matches what cranelift-jit's `finalize_definitions` does today.
  `Tests/load-memory-image.c` performs the same steps in C, out of process,
  as the reference the Jet loader is checked against.

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
| `os_copy_in(address, bytes)` | Copy a `[U8]` in, one little-endian word at a time |
| `os_call_address(address, argument) -> Int` | Call-code-address (below). `#[Unsafe, FFI(asm)]` |
| `os_library_open(path)`, `os_library_symbol(library, name)` | Bind runtime exports by name through the dynamic linker (today's runtime only, below) |
| `os_page_round(size)`, `OS_PAGE_SIZE`, `OS_PROT_*`, `OS_SYS_*` | Helpers and constants |

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
interpreted unit (`Tests/GoldenLower.jet` plus a decoder generated from the
schema), links each object with `JET_RUNTIME_C_LIB` and compares stdout with
the golden. `<outdir>/summary.txt` counts pass, wrong, unsupported and harness
verdicts and ranks the blockers by how many goldens each one stops.
