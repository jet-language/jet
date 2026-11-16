# Jet memory management review (2026-10-23)

Owner question: are there gaps in the memory model, deficiencies, inefficiencies, improvements, or things to remove? This is a read-only review by an Opus agent; findings are dated evidence, not current status.

Tree: `.agent-worktrees/Main-dev-collectall` at dev 8e7747df75. No builds, runs, or edits were done.
Tags: **VERIFIED** means I read the code at the cited file:line. **INFERENCE** means a reasoned conclusion that no run has confirmed.
Paths are abbreviated: `RT/` = `Compiler/JetRuntime/Source/`, `IR/` = `Compiler/JetIR/Source/`, `SEMA/` = `Compiler/JetSema/Source/Sema/`.

## 1. Model in brief (how it works today)

- **Access modes.** A parameter is read (unmarked), write (`&`), or take (`^`). The last use of a value moves it. A use that is not the last one *shares* the value; `.copy()` is explicit. Views carry provenance (E2305/E2307). (VERIFIED, Docs/spec/spec.md:1003-1046.)
- **Sharing is refcount plus copy-on-write.**
  - Every heap block with align ≥ 8 has a count word at payload-24 (RT/Alloc.jet:114-126; RT/Refcount.jet:54-64).
  - Writers call `ir_make_unique` / `ir_box_make_unique` (IR/BuildOwn.jet:358, 519).
  - Copies retain the buffer through `ir_copy_fixup` (IR/BuildOwn.jet:54-80).
  - Retain and release use LOCK XADD. They are inlined on x64 (IR/X64Select.jet:855-887) and also exist as routines (RT/Refcount.jet:29-50). (VERIFIED)
- **Boxing.** Every Option, every Result, and every enum with any payload is a heap box: discriminant at 0, payload from 8 (VERIFIED, IR/BuildTypes.jet:170-181, 276-285). Strings use a separate header block `[count][data,len,cap]` (RT/Text.jet:54-65).
- **Drops.** Drop glue is `ir_drop_glue(s)` (IR/BuildOwn.jet:425-441). Close glue walks List/Map elements, Option/Result, enum payloads, and struct members (IR/BuildOwn.jet:695-903). (VERIFIED)
- **Allocator.** An mmap heap with no libc (VERIFIED, RT/Alloc.jet):
  - Power-of-two classes from 32 to 4096 in 64 KiB slabs, carved from 32 MiB regions.
  - 32 futex-locked arenas, picked by stack band (195-258).
  - A per-thread cache in the TCB: 8 classes, refill 32, drain at 64 (489-562, 960-986).
  - Large blocks use mmap classes with at most 25% slack and a 64 MiB reuse cache (564-651).
  - A checked mode, `JET_CHECKED_ALLOC=1` (1013-1306).
  - A counting tracker (678-846).
- **Process exit (#5478).** `main_process_ending` makes `jet_rt_free` a no-op (RT/Alloc.jet:938-939) and makes `rc_release` return false (RT/Refcount.jet:44-50). User Close still runs, then `secret_scrub` runs (RT/Main.jet:28-37, 55-59, 182). (VERIFIED)
- **Comptime runs on the same tier.** Comptime is compiled to x64 at O0 and runs in a child process on the same runtime image (VERIFIED, Compiler/JetEval/Source/IR/Native.jet:1-21). So comptime and run share one memory model; this is good for I9.
- **Web.** No web code generator exists under `Compiler/`. Web has only a JS host (`Prelude/Web`). Memory parity with web is therefore not yet testable. (INFERENCE from glob.)

## 2. Gaps (missing or unsound)

G1. **The native backend cannot build the expert memory tier at all.** (VERIFIED)
   - `TExprKind.AllocNew` (arena/bump/pool allocation, SEMA/Calls/Memory.jet:215) has no case in JetIR.
   - `Deref`, `RawAddressOf`, and `PtrFromAddr` have no case in JetIR either; they appear only in IR/Names.jet:58-60.
   - All four fall through to `ir_fail_slot(..., "expression {kind}")` (IR/BuildExpr.jet:1426-1428).
   - `Core/Mem/Mem.jet:23-31` declares `Arena {}`, `Bump {}`, `Fixed {}`, `Pool<T> {}`, and `Pin<T> {}` as empty structs.
   - `mem.from_addr` panics with "INTERIM card #4971" (Mem.jet:34-38).
   - spec.md:1817-1946 describes all of this as working.
   - Cards: #5535 (E2104 "can't build … AllocNew"), #5534 (Arena is not Close), #5536 (missing arena escape checks), #4971.
   - A related I2 point (INFERENCE): checked code that the backend rejects should be an ICE, not a user diagnostic.

G2. **Stack and arena sentries are never emitted.** (VERIFIED)
   - IR/BuildExpr.jet:464-466 lowers `.SentryPolicy`, `.Unsafe`, and `.UnsafeBlock` to plain scopes.
   - `uses_stack_sentry` is always false (IR/BuildRoutes.jet:822, 920; BuildZip.jet:279; CoreRecords.jet:193).
   - JetIR contains no `jet_rt_sentry_*` call.
   - RT/Sentry.jet (555 lines) is reachable only from MemStorage (itself unreachable, see G1) and from RT test fixtures.
   - The spec promise that an address into a dead frame faults with R0802 (`Examples/features/memory/unsafe_sentries.jet`) cannot hold natively.
   - Cards #4889 and #4894 are related.

G3. **`#Context(...)` blocks fail in the backend.** (VERIFIED)
   - `.ContextBlock(_, _, span) -> ir_fail(&b, span, "statement ContextBlock")` (IR/BuildExpr.jet:477). `#Context(allocator:)` and `logger:` therefore cannot even build natively.
   - `.Layout` fails the same way (line 476).
   - Card #5030 ("allocator has no effect") understates the problem.

G4. **Scoped GC (`#Policy(gc)`, D-OPTGC1) exists only in sema.** (VERIFIED)
   - Sema computes `GCPromotion` facts (SEMA/FunctionMemory.jet:476-881; SEMA/Statements.jet:3320-3324).
   - JetIR ignores both fields: `.Let(local, let_ty, init, _, _, span)` (IR/BuildExpr.jet:224).
   - No JetIR file mentions GC.
   - `Examples/features/memory/gc_cyclic.jet` only builds value trees with `.copy()`, so it passes the same way with or without GC. It is a vacuous proof (I5).

G5. **Two Secret mechanisms with different guarantees (I8, I1).** (VERIFIED)
   - The runtime Secret (RT/Secret.jet:113-167) wipes len plus spare capacity, registers in the crash-scrub registry, and applies mlock/MADV_DONTDUMP. Vault (VaultState.jet:29-31) and TLS (TLSCarriers.jet:179) use it.
   - `core.crypto.Secret` (Core/Crypto/Crypto.jet:168-196) calls `__zeroize` → `jet_crypto_zeroize`, which wipes only `len` bytes (CryptoEntropy.jet:78-84). It does not register for the fault/abort scrub.
   - SigningKey, X25519SecretKey, and Hasher repeat that pattern (Crypto.jet:154-158, 217-222, 256, 287).
   - INFERENCE: List growth (push/realloc) frees old buffers without wiping them, so copies of key material stay in freed slabs.

G6. **Possible use-after-free in `jet_rt_secret_from_bytes`.**
   - VERIFIED: it adopts the List buffer without checking that the count is 1 (RT/Secret.jet:116-135).
   - INFERENCE: if sema lets a shared buffer reach this call, Close zeroes and frees memory that another holder still reads. Sema's access rules should be confirmed, or the call should make the buffer unique.

G7. **Close glue ignores buffer sharing.**
   - VERIFIED: `ir_close_parts` closes every List/Map element regardless of the buffer's count (IR/BuildOwn.jet:814-835).
   - INFERENCE: this is sound only if sema forbids sharing values that hold Close types (spec.md:1095-1098, 1133-1134). No check of this was found.

G8. **Inline release ignores process exit.** (VERIFIED)
   - `ir_x64_block_release` returns true at count 1 and never checks `main_process_ending` (IR/X64Select.jet:869-887). `jet_rt_rc_release` does check it (RT/Refcount.jet:47-49).
   - Result: inlined sites still walk drops after exit begins. The frees are no-ops, so this is wasted time rather than unsound, but it contradicts RT/Main.jet:31-33 ("no drop walks … on any tier").

G9. **Loaded-image hosts leak.**
   - VERIFIED: `main_process_ending` is cleared only in RT/Main.jet:179-183.
   - INFERENCE: each entry run in a long-lived host (fn dev/build child, Native.jet:17-21) abandons its live heap.

G10. **Open soundness cards (not re-verified here).**
   - P0: #4911 (move out of a read guard), #5057 (nested payload closed before it is consumed), #5058 (`^x` of a borrowed place silently deep-copies).
   - #3917 (pins, exclusive windows, and partial moves unchecked), #4959, #4718.
   - Missed drops: #5517, #5581, #4703, #5059. Field drop order: #4617.

## 3. Deficiencies and inefficiencies (code evidence; no measurements taken)

D1. **Every Option, Result, or payload enum is a heap allocation plus an atomic count.**
   - VERIFIED: IR/BuildTypes.jet:170-181, 276-285.
   - Option<Int> is 16 bytes, plus a 24-byte header = 40 bytes, which rounds to the 64-byte class (RT/Alloc.jet:100-118).
   - Card #5469 measured 3M pops at O0.
   - Rust stores these inline or with niches. This is the single largest gap against "Rust-or-better" (#4620).

D2. **Allocator headers and power-of-two classes waste space.** (VERIFIED)
   - Each align-8 block carries `[count][slab][block-start]`, 24 bytes (Alloc.jet:114-118).
   - Classes are powers of two (105-111), so internal fragmentation approaches 2x.
   - A String header block (32 bytes payload) takes a 64-byte block.
   - Payloads above about 4072 bytes take the large mmap path, with a 2-page minimum, a syscall on a cache miss, and a global lock (581-651).

D3. **Duplicate count word on Strings (I8).** (VERIFIED)
   - The String header block has Text's own count (RT/Text.jet:59-63) and the allocator's unused count at payload-24 (Alloc.jet:121-126).

D4. **Three refcount layouts.** (VERIFIED)
   - The allocator block count (Refcount.jet).
   - The Text prefix count.
   - The packed owners/pins plus weak count on the 96-byte Shared cell, updated with a CAS loop (RT/Shared.jet:4-24).

D5. **Atomic refcounting even in single-threaded code.** (VERIFIED)
   - Retain and release always use LOCK XADD (Refcount.jet:29-50; X64Select.jet:855-887).
   - Shared<T> takes a mutex on every field access (spec.md:1306-1309; Shared.jet:30-42).

D6. **The counting tracker scans a linked list.** (VERIFIED)
   - `alloc_untrack` does an O(n) scan under the policy lock on every free (Alloc.jet:802-829), so counting mode is O(n²).

D7. **The Secret registry is costly and has side effects.** (VERIFIED)
   - Register and unregister scan linearly (RT/Secret.jet:35-74). Registry pages are never freed.
   - mlock/MADV_DONTDUMP are applied to whole slab pages that other blocks share, and are never undone (Secret.jet:100-111). This uses up RLIMIT_MEMLOCK and hides unrelated data from core dumps.

D8. **The heap never shrinks.**
   - VERIFIED: empty slabs keep their pages (Alloc.jet:289-295). Long-running servers keep their peak RSS.
   - Compiler-memory cards: #5013, #4641, #4585.

D9. **Unchecked frees.**
   - VERIFIED: free trusts the caller's size and align. Only checked mode validates them. Freed blocks are not poisoned.
   - #5093 asks for the checked allocator to gate the corpus.

D10. **Stale or dangling comments.** (VERIFIED)
   - Alloc.jet:172 says "32 * 200 bytes"; rows are 216 bytes (Alloc.jet:8).
   - Comments cite deleted Rust files: `ProgramAllocator.rs` (Alloc.jet:60, 738-739, 756, 782), `MemSentry.rs` (Sentry.jet:320), `SharedProtocol.rs` (Shared.jet:1), `Prelude/Mem.rs` and `FixedAllocator.rs` (MemStorage.jet:1-2), and `Core.rs`/`CAbi.rs`/`EnvInit.rs` (Main.jet:1-2, 42, 83, 175, 186).
   - A glob finds none of those files.

## 4. Improvements (ranked by value / cost)

1. **Fix G6 in `jet_rt_secret_from_bytes`.** Make the buffer unique before adopting it, or assert that its count is 1. Tiny change; closes a possible UAF.
2. **Fix the inline-release exit check (G8).** Add the flag check to `ir_x64_block_release`. Tiny; the exit path then matches its own comment.
3. **Merge core.crypto Secret onto the runtime Secret (G5, I8).** Wipe capacity and register for scrub. Small.
4. **Tell the truth in the spec.** Mark spec.md:1817-1946 (arenas, pointers, sentries) and D-OPTGC1 as unbuilt on the native backend, or make G1/G2 a P0 build. Doc-only at minimum.
5. **Unbox Option/Result/small enums (D1, #5469).** Largest performance win. Medium cost: the layout change touches the close/copy glue.
6. **Align slabs to 64 KiB (D2).** Then `slab = ptr & ~0xFFFF` and the `[slab][block-start]` words can go for naturally aligned payloads, cutting the header from 24 to 8 bytes. Add 3·2^k classes (48, 96, …) and slab classes up to about 16 KiB. Medium cost.
7. **Drop Text's duplicate count (D3).** Reuse the allocator count. Small-to-medium cost.
8. **Give Secret storage its own pages (D7).** Unlock pages on Close, and use a hash or free-slot registry. Small-to-medium cost.
9. **Use non-atomic counts for thread-confined blocks, or biased counts (D5).** Ties to #4620/#5041 and the D-MEMREF-TASK1 `#Local` ruling. Medium cost.
10. **Return empty slabs and idle large blocks to the OS with MADV_DONTNEED (D8).** Small-to-medium cost.
11. **Replace the counting-tracker list with a hash keyed by address (D6).** Small cost.
12. **Run the golden corpus under `JET_CHECKED_ALLOC=1` in CI (#5093).** Small cost.

## 5. Remove or merge

- **Merge Secret (I8).** Keep only `RT/Secret.jet` and make `core.crypto.Secret` a thin surface over it (G5).
- **Unreachable runtime code.** `RT/MemStorage.jet` (468 lines), MemRegistry.jet, and Sentry.jet (555 lines) are unreachable from generated code (VERIFIED by grep: no JetIR caller). Either wire them up as part of G1/G2, or delete them along with the empty `Core/Mem` structs.
- **Sema-only GC planning.** The planning in SEMA/FunctionMemory.jet and the `gc_promotion`/`gc_transferred` fields on every `Let` (AST/Statements.jet:166-167; TIR Tree.jet:221) are dead weight until a collector exists. Either build it or remove it, together with the vacuous `gc_cyclic` example.
- **Overlapping identity mechanisms (I8).** View, ViewMut, Shared, Cell, Pin, Pool/ID, and `#Policy(gc)` overlap. D-MEMREF1=A (`@` links) is ratified to replace View, ViewMut, Shared, Cell, and Pin, but nothing of it is built (Docs/proposals/memory-model-2026-10-05.md:140-170; syntax-decisions.md:34). The old spellings remain the only built ones.
- **Rust-era leftovers.** Dangling Rust citations (D10). `tests/*.rs` allocator cases are retired in Tools/testkit/retired-with-rust.tsv:6-7, and the lost coverage is tracked by #4945.
- **Possibly stale cards to recheck:**
  - #5042 (global CAS spin lock): the code now has 32 futex arenas plus thread caches.
  - #4749 ([U8] stored as 8-byte words): `RT_HOST_U8_STRIDE :: I64{1}`, and `ir_list_stride` uses the true size (BuildTypes.jet:2893-2896).
  - #4792 and #4767: they cite the deleted `JetBackend/Source/Lower`, and JetIR close glue handles generic instances (BuildOwn.jet:780-785).
  - #5030: G3 is now broader than the card.
  - #4911: it cites `jet_sentry_read`, a Rust-era name.

## 6. Owner decisions needed

1. **Expert memory tier (Arena/Bump/Fixed/Pool/Pin, raw pointers, sentries):** build it now as P0, or mark it unbuilt and hide the surface? (G1, G2; #5534-#5536, #4971)
2. **D-MEMREF1 `@` links:** start the cutover, which retires View, ViewMut, Shared, Cell, and Pin, or reaffirm the current mechanisms? Five built mechanisms plus one unbuilt ratified replacement violates I8.
3. **D-OPTGC1 scoped GC:** build a collector or retire the ruling and the sema code (G4)?
4. **`#Context(allocator:/logger:)`:** build it or remove it (G3, #5030)?
5. **Boxing policy for Option/Result/enums:** inline layouts are a large ABI and glue change (D1, #5469).
6. **Atomic versus local refcounts by default,** and whether to accept biased counting (D5).
7. **Secrets:** dedicated mlocked pages (costs a page per secret class), or drop mlock (D7)?
8. **Loaded-image host heap policy:** reclaim the heap between entries, or accept the leak (G9)?
9. **Ballot #4935** (take/replace out of a borrowed field).
