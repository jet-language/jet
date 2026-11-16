# Memory chokepoint adversarial design review (2026-11-14)

This review covers the first criterion of card #6842. On 2026-11-14 the owner asked that every raw memory action go through one small module that can be audited heavily. Structure would then prevent memory bugs, and any memory-safety risk could be found by looking in one place.

The proposal claims five strengths:

1. **Structural prevention.** Memory bugs become hard to write, not just easy to find.
2. **One audit spot.** A reviewer reads one module to see every memory-safety risk.
3. **Zero run-time cost.** The kernel's primitives inline at O1 and O2, so a program pays nothing for going through it.
4. **Easy for beginners.** Beginners never meet raw memory.
5. **Power for experts.** Experts can still write allocators, intrusive structures, device drivers and lock-free code.

The review compares each claim with the strongest practice in use today:

- Rust's unsafe code, concentrated in the standard library and audited crates, with the unsafe-code guidelines and Miri;
- seL4's verified capability kernel;
- CHERI's hardware capabilities;
- Zig's explicit allocators;
- Go's runtime-only `unsafe`;
- Swift's `UnsafePointer` family, `Span` and strict memory safety;
- Java's foreign memory API;
- Tock's unsafe-free capsules.

The review ran in two passes:

1. **The author's pass.** It attacked each claim with probes on dev `9f193bc6c3`, gave verdicts, redesigned away the downsides it found, and reviewed the redesign. Its redesign was a kernel that hands out single-use handles.
2. **A fresh adversarial pass.** A second agent, with no stake in the first pass, reran the load-bearing probes on dev `3c7b390444`. It then attacked every option on these points:
   - same-program fairness of the timings;
   - hidden costs;
   - whether handles stop a read after free in release builds;
   - foreign calls, atomics and concurrency;
   - generated code and the migration;
   - the beginner and expert paths;
   - options the first pass missed.

   The fresh pass found that single-use handles can be copied, both directly and inside a struct. A release build then reads freed memory without a word. It also found that Jet's existing view checker already refuses every read after close that the probes tried. The redesign below therefore hands out owners and checked views instead of single-use handles. Where the fresh pass moved a number or a verdict, the text says so.

Probes ran on Linux x86-64 with the lane's gate `jet` (a0.0.45-dev). They ran one program at a time, pinned to cores 16-31, with timings on one core. The probe sources, the scripts that rerun them and their full output travel with this review's submission. Every gap ends in the gap table with the card criterion or ballot that owns it. The owner's choice is ballot D-MEM-CHOKEPOINT1.

## Measured facts

The census script reads the region list that `Tools/agent/UnsafeRatchet.jet` keeps in `Docs/spec/safety.md`. It cuts each region's text out of its source and tags it with the memory actions it performs; one region can carry several tags. The tags come from patterns, not from sema, so they are approximate. A hand check of 20 random rows found two misses (`Core/Testing/Testing.jet:1389` reads through `rt.string_byte`; `Core/Mem/Mem.jet:196` is `Region.adopt`). Both were fixed before the final run. The fresh pass reran the census on `3c7b390444` and got the same tables, apart from the order of tied compiler wrappers.

| # | Probe or read | Observed |
|---|---|---|
| M1 | Ratchet list in `Docs/spec/safety.md` | 1528 `#Unsafe` regions: the compiler proper 66, the OS layer (`Compiler/JetBackend/Source/OS`) 75, the runtime (`Compiler/JetRuntime`) 650, Core 181, Jetpack 8, and tests, Examples, Docs and Tools 548. Compiler/ totals 791, as the brief said. |
| M2 | Census, one bucket per region | 656 regions address memory directly (allocate, free, resize, raw read or write, pointer step, byte copy, atomic, OS mapping, foreign crossing or asm). 170 more only lend a buffer to a system call. 110 only write global runtime state. 592 do no memory action at all. Of Core's 181 regions, only 25 address memory; the other 156 are crypto expert entries, provider boundaries, process control and private carriers. |
| M3 | Census, per action (multi-label) | Raw read or write 474 regions, of which the runtime holds 320. Pointer step 76, mostly in tests. Allocate 34, free 11, resize 5, byte copy 17, atomic 19, OS mapping 39, foreign crossing 45, asm leaves 88 (67 in the OS layer). *Fresh pass: the first draft printed 471 and 33; the census output says 474 and 34.* |
| M4 | Census, second pass: safe functions whose own parameter reaches a raw access inside their gate | The runtime has 248 such functions, the compiler 15, Core 1 (`mem.Window`'s checked `get`, which has a proof row) and Jetpack 1. Their names appear 17,528 times in the runtime. 42 of the runtime's are one-line wrappers of the same machine load or store, such as `sync_load`, `rt_text_word`, `trap_word`, `fd_word` and `dyn_word`. Each wrapper's reason ("the caller borrows an initialized word…") is a claim about its callers, and no caller opens a gate. The busiest are `rt_text_word` (1963 uses), `sync_load` (1862), `rt_text_store` (1450) and `sync_store` (1207). |
| M5 | `Core/Mem/Mem.jet`, `Compiler/JetIR/Source/Build.jet:956-976`, `Compiler/JetBackend/Source/OS/Linux.jet` | The raw primitives are already defined in three places. `core.mem` is the user surface: `from_addr`, `offset`, `byte_offset`, `cast`, `alloc_raw`, `free_raw`, `copy`, `fill`, `window`, `volatile_read`/`volatile_write` and `stream_store`. The IR builder lowers them through 17 `core.mem` rows of `IR_CORE_INTRINSICS`: steps, copies, fills and volatile accesses become inline IR, and allocation becomes a runtime call. `Linux.jet` calls itself "the one audited place where Jet code talks to the kernel directly"; its `#[Unsafe, FFI(asm)]` leaves (`os_load_i64`, `os_store_i64`, `os_cas_i64`, `os_swap_i64`, `os_fetch_add_i64`, `os_syscall_i64` and others) are what the runtime uses. What is scattered is not the primitives but the reasoning at each use. |
| M6 | `Compiler/JetSema/Source/Sema/UnsafeObligations.jet:395-407` | Inside a gate, sema proves nothing. It names each raw operation's obligations (`valid_ptr`, `aligned`, `no_alias`) and, under the `.Obligations` policy, checks that the author wrote `assert` with those names. What sema does refuse today: a second `free_raw` of the same binding (E0209), and any use of a moved value. |
| M7 | P2: `kept.jet` (from `tests/cases/unsafe_and_runtime`) reads through a copy of a pointer after `free_raw` | It compiles under every profile. `jet run`, plain `jet build` and `--profile=hardened` all stop with `Runtime fault [R0802]` at `kept.jet:10:19`. `--profile=release` prints the stale `5` and exits 0. *Fresh pass: reproduced exactly.* |
| M8 | P1: a loop of 300 million reads, written as `words.offset(i).*` (direct) and through a generic `#Unsafe` `kernel_read<T>(base, index)` (kernel), `--profile=release` | For `I64` elements both images are 92,144 bytes and differ in 67 bytes: the file name, the build ID and the order of one load against the preemption poll. Median wall times: direct 153 ms and kernel 161 ms in the first pass; 130 ms and 131 ms in the fresh pass. With `--inline=off` the kernel costs 2.6× (407 ms) in the first pass and 2.7× (357 ms) in the fresh pass. For `Int` elements the generic kernel calls `jet_rt_int_clone` on every read (`--opt-dump=sum_words`): +45% in the first pass (359 against 248 ms) and +55% in the fresh pass (310 against 200 ms). Under plain `jet build`, with sentries on, the same `I64` loop takes 6.1-6.9 s, about 40× the release time. |
| M9 | P3: a `#SingleUse Words` struct with `priv base` and `priv count`, made only by `acquire` and spent only by `release(words: &&Words)`, used from another file | A forged `kernel.Words{base: 4096, count: 1}` is refused (E0605). A read after `release` is refused (E0121). A dropped capability bound by a plain binding is refused (E0140). Passing a capability to any function by `&` or by plain parameter is refused (E0142 "can't be shared"), so a capability can only be used through its own methods. A capability bound by `if kernel.acquire(4) == .Val(words)` and never released compiles and runs (exit 0). When `release` took `Words` by borrow instead of `&&`, the read after release compiled and only the sentry stopped it (R0802). *Fresh pass: the first draft also said "a copy is refused (E0142)". The probe behind it, `copy.jet`, binds `kept: words`, which is a move, and the E0142 is the later lend. An explicit copy compiles (M15).* |
| M10 | P4: a package with `policy: { unsafe: .Paths(["kernel.jet"]) }` | `#Unsafe` in `run.jet` is refused: "Package policy `policy.unsafe` rejects `#Unsafe` at `run.jet`" (E2960). But `run.jet`, in the same package and with no `#Unsafe`, can write `Words{base: 4096, count: 1}` and pass it to `release`. `jet check` says no problems. `jet run` stops with R0801; the release build dies with SIGSEGV at address `0xff0`. A package's files share one module, and `priv` hides a field only outside its type's module. *Fresh pass: reproduced exactly.* |
| M11 | P5: the same 300-million-read loop, written in `run`, through the P3 capability's bounds-checked `read`, `--profile=release` | First pass: 563 ms in one file and 583 ms with the capability in another file, 3.7× the raw read. Fresh pass: 470 and 472 ms, 3.6×. The images match in size and in inline count (11), so crossing a file costs nothing. Without the bounds check the method costs 1.85× (240 ms). A safe `[I64]` loop costs 1.4× (183 ms), including the pushes that build it. Through `mem.Window` it costs 15× (1980 ms): `--opt-dump=run` shows a `jet_rt_alloc` and a free for every element read. *Fresh pass: these ratios measure the loop's shape as much as the check; see M21.* |
| M12 | `Core/Mem/Mem.jet:177-264`, `Core/Sys/Sys.jet:1466-1546` | Core already holds memory in three typed shapes. `mem.Block` is `#SingleUse` with private address and size. `mem.Region` is a `Close` resource that owns its pages, and it cannot be copied (E0211). The shared-memory word API checks a token and a bounded offset before each atomic. |
| M13 | `Compiler/JetRuntime/package.jet`, `tests/ui/unsafe_per_site`, Jetpack policy tests | The runtime package sets no `policy.unsafe`. The policy has `.Paths([...])` and `.Deny` (an empty list); `.Allow` is refused, so package policy can only tighten (D-TEAMPOLICY1). |
| M14 | `Compiler/JetIR/Source/OptInline.jet:1-53` | A callee inlines when its cost fits the level's limit (40 at O1, 60 at O2) and the caller's budget (200 and 400). `#Inline(Always)` lifts the size limits. Asm leaves are never inlined as IR; the x64 selector splices inlinable asm in place (`X64Select.jet:712-758`). |
| M15 | Fresh pass, `adv/a10_copy.jet` and `adv/a11_block_copy.jet`: `words.copy()` on the P3 capability, then `release` of both; `block.copy()` on Core's `mem.Block` | Both compile with no copy error. `jet check a10_copy.jet` reports no problems. `jet run` stops at the second `free_raw` with R0802. The release build frees the same memory twice and exits 0 with no message. For `mem.Block`, the only errors are E0140 on each of the two copies: each copy owes its own `consume`, so consuming both passes the checker. `#SingleUse` tracks a duty per binding; it does not forbid copies. |
| M16 | Fresh pass, `adv/a3_struct.jet`, `a3b_struct_copy.jet`, `a3c_struct_twice.jet`, `a9_block_copy.jet`: a `#SingleUse` value stored in a field of an ordinary struct | The struct is dropped without E0140, so the capability leaks (exit 0). The struct can be copied with `.copy()`. With `h2: h.copy()`, `kernel.release(h.w)` and then `print(h2.w.read(0))`, `jet check` reports no errors. `jet run` stops with R0802. The release build prints the stale `7` and exits 0. The user file holds no `#Unsafe`. The same holds for `mem.Block` in a struct: the two copies report the same address. |
| M17 | Fresh pass, `adv/a1_view.jet`: the capability hands out a `mem.Window<I64>` through a method; the window is read after `release` | It compiles. `jet run` stops inside `Window.get` with R0802. The release build prints the stale `7 7` and exits 0. `mem.Window` is a plain copyable struct, so no rule ties it to the capability it came from. |
| M18 | Fresh pass, `adv/a2_borrow.jet`, `a4_closure.jet`, `a5_generic.jet`, `a6_region.jet`, `a7_leak2.jet`, `a8_leakif.jet` | A capability passed to a helper by plain read-only parameter is refused (E0142), and so is one passed as a generic `T` (E0142). This is ratified behaviour: D-LIN1 says a single-use value is refused when lent. A closure that captures a released capability is refused (E0121). Copying a `mem.Region` is refused (E0211). A plain binding that drops a capability is refused (E0140); a pattern binding that drops one compiles and exits 0. |
| M19 | Fresh pass, `views/e1_lend_resource.jet`, `e2_struct_resource.jet`, `e7_autoclose.jet`: a `Close` resource | It can be lent to a helper by plain parameter and is still closed at the end of its scope (prints `6`, then `closed`). A struct that holds one cannot be copied (E0211 "`Holder` can't be copied"). A `Close` resource bound by a pattern binding is not closed at the end of its scope; only the plainly bound one prints `closed`. |
| M20 | Fresh pass, `views/e3`, `e4`, `e8`, `e10`, `e11`, `e12` and `tests/ui/arena_view_after_reset.jet`: Jet's view checker | A `View<T>` returned by an owner's method is tied to that owner. Growing the owner while the view is live is refused (E0212), and so are moving it into a `&&` parameter and closing it (E0212, with the view returned from a method in e8 and e10). Returning a view of a function's own local owner is refused (E2305 ViewOutlivesOwner, e11). Arena views are refused after a reset (E0632) and when they escape (E0631). One hole: a `View<I64>` put into a field of a returned record escapes its owner's function and compiles (e12). With a list behind it, it prints `1` under `jet run` and in a release build. |
| M21 | Fresh pass, `fair/`: the same 300-million-read sum, every program built `--profile=release` and timed five times on one core, medians | Raw read in its own function: 130 ms. Raw read with the loop in `run`: 130 ms. Safe `[I64]` loop in its own function: 74 ms. `View<I64>` from an owner's method, loop in its own function: 123 ms (0.95×). The P5 capability with the counted loop in one of its methods: 187 ms (1.4×); `jet inspect decisions` shows "stepped index sum: guarded before the Loop". The same view with the loop in `run`: 239 ms (1.8×). P5 in `run`: 470 ms (3.6×), and the same with the count hoisted out of the loop. Only the raw read is unaffected by where the loop sits. |
| M22 | Fresh pass, census `regions.tsv` | The runtime's 387 addressing regions sit in 81 of its 385 source files, which hold about 36,600 lines. A runtime kernel drawn around today's raw code would be those 81 files. |

The P5 opt-dump of the check-free loop in `run` (`fair/nocheck-dump.txt`, from `probes/p5/nocheck.jet`) shows the capability struct reloaded from memory on every iteration, the slow path through `jet_int_owned_to_i64`, and a `jet_rt_int_clone` of the count on every outer iteration. The decisions also show "stepped index run: guarded before the Loop". So the first draft's explanation, that the length is not a range fact, is not the whole cause of the 3.6×. The loop in `run` pays for its shape, and every non-raw access pays it.

Sema-proven accesses, such as indexing a list inside a counted loop, are not `#Unsafe` regions at all. The IR builder emits their loads directly, and `jet inspect decisions` shows the proof per program ("stepped index run: guarded before the Loop"). A repo-wide count of them needs the compiler built over the repo, which this review did not do. The generated inventory of criterion 2 should count them from the IR.

## Peer practice

**Rust.** Rust marks raw operations with `unsafe` and asks authors to hide them behind safe APIs. Most unsafe code lives in the standard library and a few audited crates. Evans, Campbell and Soffa found the keyword in under 30% of crates, but more than half of crates reach unsafe code through a dependency (ICSE 2020, arxiv.org/abs/2007.00752). Astrauskas et al. found most unsafe code "simple and well-encapsulated", yet used widely, especially for foreign calls (OOPSLA 2020, doi.org/10.1145/3428204). The unsafe-code guidelines define what unsafe code may assume (github.com/rust-lang/unsafe-code-guidelines). Miri runs tests in an interpreter that catches out-of-bounds reads, use after free, misalignment and aliasing violations (github.com/rust-lang/miri). RustBelt proved key library abstractions sound (plv.mpi-sws.org/rustbelt). The Rustonomicon states the limit plainly: changing a `<` to `<=` in safe code makes a function unsound, so "safety is non-local", and "the only bullet-proof way to limit the scope of unsafe code is at the module boundary with privacy" (doc.rust-lang.org/nomicon/working-with-unsafe.html). What a safe API hands out is an owner (`Vec<T>`) and borrowed slices (`&[T]`) that the borrow checker ties to it, at no run-time cost. A `Vec` cannot be freed while a slice of it is live. Rust's strength is the safe-abstraction discipline plus privacy plus checked borrows; its gap is that nothing forces unsafe into a few modules.

**seL4.** seL4 is a microkernel of 8,700 lines of C and 600 of assembly with a machine-checked proof of functional correctness (Klein et al., SOSP 2009). Every kernel object is reached through an unforgeable capability, and the kernel checks each one. Its strength is the true single chokepoint: a small kernel, proven, with authority carried by capabilities. Its cost was years of proof work and a call into the kernel for every privileged action.

**CHERI.** CHERI replaces pointers with hardware capabilities that carry bounds and permissions and a validity tag that software cannot forge (cl.cam.ac.uk/research/security/ctsrd/cheri). Spatial safety comes from every load and store, not from one module, and temporal safety needs a revocation sweep on top. Its strength is that no code, however careless, can step outside its bounds. Its cost is wider pointers, porting and special hardware such as Arm's Morello board.

**Zig.** Zig has no hidden allocations: a function that allocates takes a `std.mem.Allocator`. The debug allocator reports leaks, double frees and use after free, and Debug and ReleaseSafe builds keep bounds checks (github.com/ziglang/zig/blob/master/lib/std/heap/debug_allocator.zig). Raw pointers work anywhere, with no keyword. Its strength is that allocation is explicit and swappable. Nothing gathers the raw operations into one place.

**Go.** Go code that converts pointers must import `unsafe`, and that import is easy to search for (pkg.go.dev/unsafe). The documented `unsafe.Pointer` conversion patterns are the rules. `go vet` flags misuse, and `-d=checkptr` checks conversions at run time; `-race` and `-msan` turn it on (go.dev/src/runtime/checkptr.go). The garbage collector removes use after free from safe code. Its strength is that ordinary programs never need `unsafe`. Its gap is that the rules are checked by tools, not by the type system.

**Swift.** Swift names every raw pointer type `Unsafe…` and scopes them with `withUnsafeBufferPointer` closures. `Span` (SE-0447) is a safe, non-escaping view of contiguous memory whose lifetime the compiler ties to its owner. Strict memory safety (SE-0458) makes each use of an unsafe declaration carry an `unsafe` mark when a module opts in. Its strength is a safe view type that costs nothing at run time, plus per-use marking. Its gap is that the strict mode is opt-in.

**Java's foreign memory API.** JEP 454 gives `MemorySegment`, a typed handle with bounds and a lifetime that an `Arena` controls. Every access is checked, and the JIT removes most of the checks. JEP 471 deprecates `sun.misc.Unsafe`'s memory methods for removal, so raw memory is moving into one checked API (openjdk.org/jeps/454, openjdk.org/jeps/471). Its strength is that a capability handle replaced raw access across a whole platform. Its cost is checks that only a JIT removes, and a closed arena turns later access into a run-time exception, not a compile error.

**Tock.** Tock is a Rust embedded OS. Its drivers, called capsules, may not use `unsafe`; only the core kernel and chip crates may (book.tockos.org/doc/threat_model/capsule_isolation). Privileged kernel functions also take capability objects that only trusted code can create (docs.tockos.org/kernel/capabilities). Its strength is the closest match to this proposal: an enforced boundary for unsafe code, plus typed capabilities across it. Its capsules still rely on Rust's borrowed slices for memory they are handed.

## Claim 1: structural prevention

**Verdict: it holds within stated limits. It holds only if the kernel hands out owners and checked views, and only after the holes in the gap table are closed. A kernel that just relocates the primitives fails. A kernel that hands out single-use handles fails as Jet builds them today.**

Take the card's literal design: a kernel of typed primitives with stated preconditions, and a sema refusal of raw operations outside it. M8 shows that design costs nothing at run time. But it moves the syntax, not the reasoning. A call to `kernel_read(base, index)` still needs a gate whose author vouches that `base` names live memory and `index` is inside it. P2's bug, a read through a copy of a released pointer, compiles the same way through such a kernel. The Rustonomicon's point applies directly: safety depends on the state that safe code builds, and a kernel of primitives cannot see that state.

The first pass put the facts into the type with a single-use capability: private base and count, one constructor that allocates, one destructor that frees. Forgery, use after release and a dropped plain binding are refused at compile time (M9). The fresh pass broke it in four ways, all silent in release builds:

- **A copy compiles (M15).** `words.copy()` gives a second capability with its own duty. Releasing both frees the memory twice. Core's `mem.Block`, documented as uncopyable, copies the same way.
- **A struct hides the duty (M16).** A capability in an ordinary struct field drops without E0140, and the struct copies freely. A file with no `#Unsafe` reads freed memory, and the release build prints the stale value.
- **A view outlives its capability (M17).** `mem.Window` is a plain copyable struct, so a window handed out by the capability is still readable after `release`.
- **A capability cannot be lent (M18).** D-LIN1 refuses to lend a single-use value at all, so helpers cannot take one. Making them lendable amends a ratified decision.

Each hole has a fix, but together they rebuild the owner-and-view rules that Jet already enforces elsewhere. A `Close` resource refuses copies, including copies of structs that hold one (M12, M19). It can be lent to helpers, and it is closed at the end of its scope (M19). A `View<T>` returned by an owner's method is tied to that owner: changing, moving or closing the owner while the view is live is a compile error, and so is returning a view of a dead owner (M20). That is Rust's owner-and-slice discipline and Swift's `Span`, and Jet already builds it for lists, strings and arenas. The redesign puts raw memory behind that same discipline.

Two limits remain. A view placed in a field of a returned record escapes its owner (M20, e12), and a pattern binding skips the close duty (M19). Both are sema gaps that must close first. Release builds run unwatched (M7). That is an owner-ratified carve-out (D-MEM-SENTRY1, D-HARDENED1), not a defect. It does mean that whatever the type rules do not prevent, a release build does not catch.

## Claim 2: one audit spot

**Verdict: it fails as stated. After the redesign it holds within stated limits: one generated report over a handful of named file sets. The runtime's set is large.**

A single module cannot hold every raw memory action, for three reasons:

- **Layering.** The runtime sits below Core and does not import it (`Compiler/JetRuntime/Source/Alloc.jet` implements the `core.mem` routes). So the runtime needs its own kernel, or a module below both.
- **Generated code.** The IR builder emits loads and stores for safe code: list indexing, fields, string bytes. Those come from lowering rows in the compiler, not from any source module.
- **FFI.** D-FFI-VOUCH1 puts each unchecked foreign row in one `#Unsafe` block in its own import module.

The attack also finds that today's "one place" claims do not hold. `Linux.jet` says it is the one audited place for kernel calls. Yet the runtime holds 248 safe functions that perform a raw access at an address their caller chose, used 17,528 times, with no gate at the call (M4). An auditor who reads every `#Unsafe` region in the runtime still has not seen those 17,528 decisions. The Core proof-row audit of `safety.md` catches this pattern in Core (a safe function that opens a gate needs a reviewer-written proof row), but it does not cover the runtime or the compiler.

A kernel that exports a safe function taking a raw address repeats the problem one level up. Rust's practice handles this with a rule: a function whose safety depends on its caller must be `unsafe fn`, and Rust 2024 also asks for an `unsafe` block around each unsafe operation inside one. Jet's E3103 already makes callers of an `#Unsafe` fn open a gate. What is missing is a rule that stops a safe function from taking the address.

The fresh pass adds the size problem. The runtime's addressing regions sit in 81 files of about 36,600 lines (M22). A runtime kernel is small only after the runtime's code is rewritten over owners and views, which is criterion 4's migration, not a property of the policy.

The census also shows that a memory kernel cannot be the only audit spot for `#Unsafe`. 592 regions (M2), including 156 of Core's 181, do no memory action. They vouch for crypto inputs, process credentials or provider boundaries, and they belong where they are. The 110 global-state regions are data-race obligations. So the report lists every raw memory operation, not every risk.

## Claim 3: zero run-time cost

**Verdict: it holds within stated limits. Word-sized primitives inline to the same machine code, and a checked view over an owner reads at raw speed when its loop sits in a function. It fails for bounds-checked handle reads (1.4× to 3.6×), for `mem.Window` (15×), for generic primitives over owned values, and wherever inlining is off.**

M8 is the clean case. An `I64` read through a generic kernel function compiles to an image the same size as the direct read, with the same instructions in a slightly different order, and runs in the same time. M11 shows the capability can live in another file with no loss: same image size, same inline count, same time.

The fresh pass checked same-program fairness (M21). The first draft compared a raw loop with a handle loop, both written in `run`. Raw reads do not care where the loop sits; every other access does. Measured with the loop in its own function, the checked handle costs 1.4× and a view costs 0.95×. Measured in `run`, the handle costs 3.6×, the view 1.8× and even a safe list 1.4×. The first draft's 3.7× is the worst shape, not the typical one, and most of it is not the bounds check: the check-free handle still costs 1.85× in `run`.

The attack finds five costs:

- **Inlining failure.** With `--inline=off`, the same kernel costs 2.6-2.7× (M8). Nothing in Jet today promises that a kernel primitive inlines: a callee inlines only while it fits the cost limit and the caller's budget (M14). A large caller, or a primitive that grows, silently turns the kernel into a call.
- **Generic primitives over owned values.** `kernel_read<Int>` clones each `Int` on return (`jet_rt_int_clone`), costing 45-55% (M8). A primitive written once for every `T` pays for the most expensive `T`.
- **Handle bounds checks.** The checked handle read keeps a check per element unless the loop sits in one of its methods (M21). A view does not: indexing a `View<T>` in a counted loop is guarded once, the way lists are.
- **Loop shape.** A loop written in `run` reloads the handle or view on every iteration (M21 and its opt-dump). This hits every safe access, so it is an optimizer gap for criterion 6, not a reason to prefer raw code.
- **The existing raw window is slow.** `mem.Window` boxes each element it reads on the heap: 15× (M11). The redesign retires it.

The dispatch hot spot the brief asked about does not appear. Every primitive is a static call or an intrinsic, and nothing in a kernel adds dynamic dispatch. The one dynamic path, the `#Context(allocator:)` handler of D-ALLOC-TRAIT1, already exists and is unchanged.

Sentries cost about 40× on the raw loop under the default `jet build` (M8). That is the price of the watched build, and the owner has ratified it. A kernel helps here: view accesses that sema proves need no sentry, so fewer accesses pay it.

## Claim 4: easy for beginners

**Verdict: it holds.**

A beginner writes lists, strings, records and maps. None of these needs `#Unsafe`, and none would meet the kernel. When a beginner does use a kernel type, an owner and its views behave like a list and its slices, which they already know, with the same errors (E0212, E2305). Single-use handles would add two errors a beginner meets for ordinary code: E0142 for passing the handle to a helper and E0140 for forgetting to spend it. The remaining risk is the refusal message: a beginner who copies a raw-pointer snippet from the web must be told which safe type replaces it, not only where the kernel is. Swift and Go show the right shape: the unsafe names say "unsafe", and safe programs never meet them.

## Claim 5: power for experts

**Verdict: it holds within stated limits. It holds if each package may keep its own kernel; it fails if only Core may.**

Experts need things no fixed kernel can foresee: allocators, intrusive lists, lock-free queues, device registers, mapped files, JIT buffers and SIMD over foreign memory. Tock lets each chip crate keep its own unsafe code; Rust lets any crate open `unsafe`. A Core-only kernel would send every such program to Core or to FFI. It would also reverse D-RAWPTR2, which gives users the full typed raw-pointer set behind `#Unsafe`.

Owners and views carry most expert shapes. An allocator, a mapped file or a JIT buffer is an owner with views. An intrusive list keeps its raw links inside its kernel file and hands out owners and views. Views cannot cross a task or a channel (E1102), and they last only as long as their scope, so memory shared between threads needs a different shape. Core's shared-word API shows it (M12): a shared owner with a token and checked offsets for each atomic. The 19 atomic regions (16 in the runtime) fit that shape. The 110 global-state regions of M2 are a different risk. They are data-race obligations ("runtime-init runs before any other thread exists"), not addressing, and a memory kernel does not fix them.

## The options under attack

The fresh pass attacked each option on the ballot. A is the redesign below; B is the first pass's redesign.

**A: kernel files that hand out owners and checked views.**

- *Fairness and cost.* Views read at 0.95× raw in a function and 1.8× in `run` (M21). Owner methods are ordinary inlined calls.
- *Read after free in release.* Every probed path is a compile error: closing, moving or growing the owner while a view lives (M20), returning a view of a dead owner (M20), and copying an owner or a struct that holds one (M19). Two sema gaps remain and must close first: a view in a returned record field (e12) and the pattern-binding close duty (M19).
- *FFI, atomics, concurrency.* Foreign memory enters through an `#Unsafe` adopting constructor of an owner, as `Region.adopt` does. Views cannot cross tasks (E1102), so shared memory uses a shared owner with checked atomic offsets (M12).
- *Generated code and migration.* Lowering rows stay the kernel for generated loads, and views lower to plain loads. The runtime's 42 one-word wrappers become owner methods and views over each header layout. The rest of the 656 addressing regions move into kernel files or onto owners, counted down by subsystem. Every user of `mem.Window`, Core's own included, moves to the owner-tied view.
- *Beginner and expert paths.* Beginners use lists and never see a kernel; a kernel type behaves like a list. Experts write a kernel file with `#Unsafe` inside and hand out owners and views.
- *Ratified decisions.* `mem.window`'s new owner argument amends that row of D-RAWPTR2. The default for a package without a `memory:` field is new language law, named below.

**B: kernel files that hand out single-use handles.** This is the first pass's redesign. Its four release-silent holes are M15, M16 and M17, plus the lending refusal of M18. Closing them needs four changes. Copies of `#SingleUse` values must be refused, and duties must pass through struct fields. Handles must not give out copyable views. And lending must be allowed, which amends D-LIN1. After all four, B has rebuilt the owner rules that A reuses. It still pays 1.4-3.6× per checked read, and every function that takes a handle must take it by `&&` and give it back. FFI, atomics and generated code are the same as A.

**C: a kernel of raw primitives.** Every caller still opens `#Unsafe` and vouches for the address, so P2's read after free compiles unchanged. It costs nothing at run time when the primitives inline (M8). The runtime's 17,528 wrapper calls become gated calls, so the number of places an auditor must read goes up. It fails claim 1.

**D: today's blocks plus a report.** The first draft said "no migration", but its own address rule turns the runtime's 248 address-taking wrappers into `#Unsafe` functions. Their 17,528 calls then open gates or move to typed views. It makes every raw memory operation visible in one report and prevents no new bug at compile time.

**E: Core only.** Core and the runtime are the only kernels; user code gets owners and views. It reverses D-RAWPTR2's user raw-pointer set. A user allocator, driver or memory-mapped device needs Core to ship it, or goes through a foreign vouch block. That vouch block is raw memory outside Core, so E must either ban it or admit it as a kernel. It shares A's view machinery and its gaps.

**Options considered and left off.**

- *Hardware capabilities, as in CHERI.* Every load would be checked, but Jet's targets have no such hardware.
- *Sentries kept on in release for kernel code only.* Sentries cost about 40× on a raw loop (M8), and kernel code is the hottest code. This works as a hardened test mode for kernels, which the redesign keeps, not as an option.
- *Each kernel as its own package, as Tock does with crates.* Package privacy would then close M10 with no per-file rule. It forces every package with raw code to split in two, and kernel privacy (step 2) gets the same result without the split.
- *Raw pointers tracked by sema.* Sema would follow each pointer's provenance and prove liveness at each raw use. Inside a gate, sema proves nothing today (M6), and Rust's experience with Stacked Borrows and Tree Borrows shows how hard the rules get. Views give the same guarantee to the code that uses them, with rules Jet already has.

## Redesign

The redesign keeps what the claims got right and removes each downside found above. It reuses existing mechanisms wherever they exist.

1. **Kernel files.** A package names the files allowed to hold raw memory operations: `policy: { memory: .Kernel(["src/kernel"]) }` in `package.jet`. This matches the existing `unsafe: .Paths` field (D-TEAMPOLICY1). Outside those files, a raw memory operation (dereference, step, cast, `from_addr`, `alloc_raw`, `free_raw`, `copy`, `fill`, `window`, volatile access, an asm memory leaf) is refused with a new registered diagnostic. The fix text names the safe type that replaces the operation. A loose file is its own kernel, so a single-file script with raw pointers keeps working. `#Unsafe` gates for non-memory reasons (crypto expert, process control) stay where they are.
2. **The default.** A package with no `memory:` field has no kernel files: its raw memory operations are refused, and the fix text names the field. An opt-in kernel would leave most packages where option D leaves them. This is a language rule, not a package policy value, so it does not conflict with D-TEAMPOLICY1's rule that package policy only tightens.
3. **Kernel privacy.** A type declared in a kernel file keeps its `priv` fields private to the kernel files, not to the whole package. This closes M10 without making every kernel its own package.
4. **No raw addresses across the boundary.** A kernel's public function that is not `#Unsafe` may not take an `Int`, `I64` or pointer parameter that reaches a raw operation's address. It must take an owner or a view, or be marked `#Unsafe("…")` so that callers open gates (E3103). The rule also refuses a public kernel record whose public field reaches a raw address. This closes M4.
5. **Owners.** A kernel type that owns raw memory is a `Close` resource, as `mem.Region` is. It cannot be copied, nor can a struct that holds it (E0211). It can be lent to helpers by plain parameter, and it is closed at the end of its scope (M19). Closing it moves it. Constructors that adopt outside memory (`Region.adopt`, foreign evidence carriers) stay `#Unsafe`. They are the short list an auditor reads first.
6. **Checked views.** An owner hands out `View<T>` and `ViewMut<T>` from its methods. Inside a kernel file, `mem.window(owner, pointer, count)` builds one over raw memory, behind `#Unsafe`, with `owner` as its provenance. It replaces today's `mem.window(pointer, count)` and the copyable `mem.Window<T>`. Sema's view rules then apply unchanged (D-MEMPROVENANCE2): no change, move or close of the owner while a view lives, no view that outlives its owner, and no view across a task. Two gaps close first: a view in a returned record field (M20, e12), and the close duty of a pattern binding (M19).
7. **Single-use values stay a protocol.** `mem.Block` stays `#SingleUse` as the allocator protocol of D-ALLOC-TRAIT1, not as an access path. Its copy holes (M15, M16) are fixed as sema bugs, because they break `Block` today whatever this ballot decides.
8. **Zero cost.** Kernel primitives are intrinsics or `#Inline(Always)`. Owner methods that return views are small and inline. Typed reads are restricted to plain-data `T`, or return a borrow, so they never clone. The view a kernel hands out must not box. The loop-shape cost of M21 is an optimizer gap for criterion 6. The paired two-program cell that the Performance gate requires checks all of this at O1 and O2, with both loop shapes.
9. **One report.** `jet audit memory` (criterion 2's generated inventory) lists, across the package and its dependencies:
   - every kernel file, with its line count;
   - every raw operation, with its gate reason;
   - every adopting constructor, every view constructor and every `#Unsafe` kernel function;
   - every lowering row that emits a load or store for safe code;
   - every asm leaf;
   - every foreign vouch row.

   An import module that holds a vouch row is listed as a kernel file. Sema-proven accesses are counted as proven, not listed as risks.
10. **Generated code.** The IR builder's loads and stores for safe code come from a named set of lowering rows, and the report lists them. A sema-proven access still lowers to a plain load, so the kernel adds no call.

## Second review of the redesign

The second pass attacks the redesign the same way.

- **Several kernels, not one.** In this repo the report would list Core's kernel, the runtime's kernel, the OS leaves, the lowering rows and the FFI vouch rows: about five places. Each package with raw code adds one. This is the price of claim 5 and of the layering in claim 2, and it cannot be removed. Mitigation: one generated report, kernels kept small by a line budget the report shows, and dependency kernels shown the way cargo-geiger shows unsafe use in Rust dependencies.
- **Is the address rule complete?** Outside kernel files there are no raw operations at all. Inside, everything is audited. The remaining leaks are a kernel's public safe function or public record field that carries an address, and step 4 refuses both.
- **Is the view rule complete?** The view checker refused every read after close, move or growth the fresh pass tried (M20). The one escape found, a view in a returned record field, is a gap in rules the spec already states (D-MEMPROVENANCE2 gives `View<T>` fields their owner's provenance). It must close before any raw-backed view ships, because with raw memory behind it the escape would be a read after free.
- **What views cannot express.** A view lasts as long as its scope and stays on one task. Memory with a dynamic lifetime is held by its owner, which can live in a field, a list or a map; each access takes a short view or calls an owner method. Memory shared across threads uses a shared owner with checked atomic offsets. An intrusive structure keeps its raw links inside its kernel. Each of these is a shape the kernel author writes once.
- **False positives.** The runtime's allocator entries (`jet_rt_alloc`, `jet_rt_free`) take addresses by design. Under step 4 they become `#Unsafe` kernel functions. Their callers are compiler-emitted code, which goes through lowering rows (step 10), so no user gate appears.
- **Migration cost.** The runtime holds 387 addressing regions in 81 files (M22), 150 buffer-lending system calls and 102 global-state writes, plus 42 one-word wrappers named about 17,500 times. This is the largest single migration in the plan. The wrapper pairs collapse mechanically into one owner type and view per header layout. The ratchet counts the rest down subsystem by subsystem. The cold self-compile of criterion 6 must not slow down.
- **Release runs unwatched.** A bug inside a kernel is silent in a release build. Mitigation: every kernel's tests run under `--profile=hardened` and under fuzzing (criterion 7), and the report shows each kernel's test count.
- **Random access still pays a check.** When the optimizer cannot prove an index, a view read keeps its bounds check, as a list read does. Mitigation: experts write an unchecked read inside their own kernel, where it is audited.
- **Contradictions with ratified decisions.** Two amendments, both named on the ballot:
  - `mem.window` gains an owner argument and returns a `View<T>`, which amends D-RAWPTR2's window row.
  - The `memory:` default of step 2 is new language law.

  D-RAWPTR2's other raw-pointer rows stay, confined to kernel files. D-FFI-VOUCH1's vouch rows stay in their import module, now listed as a kernel file. D-ALLOC-TRAIT1's `Block` stays the allocator protocol. D-MEM-SENTRY1's sentries are unchanged. D-LIN1 is unchanged under A. Option B would amend it to allow lending.

What cannot be removed:

| Downside | Cost | Best mitigation |
|---|---|---|
| Several audit places, not one | About five in this repo, plus one per package with raw code | One generated report; small kernels with a visible line budget |
| Release builds do not watch kernels | A kernel bug is silent in release | Hardened and fuzzed kernel tests; views shrink what sentries must catch |
| Runtime migration | 81 files of raw code and 17,528 wrapper uses to rewrite | Mechanical wrapper collapse first, then ratchet countdown |
| Views are scoped and stay on one task | Long-lived or shared memory needs an owner type with methods | Owners are stored and lent freely; shared memory uses Core's token-and-offset shape |
| Unproven random access | One bounds check per read | Unchecked reads inside a package's own kernel |

## Gaps

Every gap is a criterion of #6842, the card that owns this work (standing order of 2026-11-14: findings of one sweep share one card). Main may move a gap to an open card that already owns its area.

| # | Gap | Home |
|---|---|---|
| G1 | Safe functions that take a raw address and access it inside their own gate: 248 in the runtime, 15 in the compiler (M4). A sema refusal with a registered diagnostic. | #6842 c3 (the refusal) and c4 (the cutover). |
| G2 | The 42 one-word runtime wrappers collapse into owner types and views per header layout in the runtime kernel (M4). | #6842 c4. |
| G3 | `priv` hides a field per module, so a sibling file forges a kernel owner with no `#Unsafe`; the release build crashes (M10). | D-MEM-CHOKEPOINT1 (kernel privacy), then #6842 c3. |
| G4 | No owner-tied view over raw memory: `mem.window` returns the copyable `mem.Window<T>`, which outlives its memory (M17). The owner-tied `mem.window(owner, pointer, count) -> View<T>` replaces it. | D-MEM-CHOKEPOINT1 (amends D-RAWPTR2's window row), then #6842 c3. |
| G5 | A pattern binding skips the duties of the value it binds: a `#SingleUse` value escapes E0140 (M9, M18), and a `Close` resource is not closed (M19). | #6842 c3. |
| G6 | `mem.Window` heap-boxes each element it reads: 15× a raw read (M11). Retired by G4. | #6842 c6. |
| G7 | Accesses in a loop written in `run` reload the handle or view each iteration: a view costs 1.8× and a handle 3.6× there, against 0.95× and 1.4× in a function (M21). | #6842 c6. |
| G8 | A generic kernel read clones owned values (+45-55% for `Int`), and nothing makes kernel primitives inline (2.6-2.7× with inlining off) (M8, M14). | #6842 c6. |
| G9 | `jet audit memory`: kernels, raw operations, adopting and view constructors, lowering rows, asm leaves and vouch rows, with sema-proven accesses counted. The census script of this review is the prototype; the committed tool must be Jet and generated (M1-M4). | #6842 c2. |
| G10 | 110 runtime regions only write global state on the premise that no other thread exists yet (M2). These are data-race obligations, not memory addressing. | Out of the memory kernel's scope. Main routes them to the open card that owns concurrency safety. |
| G11 | The owner's choice among owners and checked views, single-use handles, a primitive-only kernel, today's gates plus a report, and a Core-only kernel. | Ballot D-MEM-CHOKEPOINT1. |
| G12 | `.copy()` of a `#SingleUse` value compiles and gives a second duty, so a double free passes the checker; `mem.Block` copies too (M15). | #6842 c3. |
| G13 | A `#SingleUse` value in an ordinary struct field drops without E0140, and the struct copies freely; a file with no `#Unsafe` reads freed memory in release (M16). D-LIN-CONTAINER1 covers lists, options, results, maps, tuples and generic arguments, not struct fields. | #6842 c3. |
| G14 | A `View<T>` stored in a field of a returned record escapes its owner's function (M20, e12). The spec gives such fields their owner's provenance, but sema does not enforce it. | #6842 c3; a blocker for G4. |
| G15 | The default for a package with no `memory:` field: refused, with a fix that names the field (redesign step 2). | D-MEM-CHOKEPOINT1, then #6842 c3. |

## Reproducing

From the submission's `tests` directory, with `JET` set to the gate `jet`:

- `python3 census.py <repo> regions.tsv` prints the M1-M4 tables and writes one row per region; M22 counts the runtime rows of `regions.tsv` by file.
- `probes/run-checks.sh` reruns P2, P3 and P4 and writes `probes/checks.out`.
- `probes/run-timing.sh` builds P1 and P5 under `--profile=release`, prints image sizes and inline counts, and times each program five times on one core (`probes/timing.out`). The medians in M8 and M11 come from that file.
- `run-review2.sh` reruns the fresh pass: the compile and fault probes in `adv/` and `views/` under `jet run`, release builds of the four whose bug compiles, and the same-shape timings in `fair/`. It writes `review2.out`, the source of M15-M21.
- The debug-build time in M8 is a one-off: `jet build direct64.jet` in `probes/p1`, then the binary timed by hand.
- `jet inspect decisions <file> --profile=release --opt-dump=<function>` prints the IR before and after optimization, used for M8, M11 and M21.
