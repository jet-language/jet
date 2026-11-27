# Zero-magic memory trio — pre-build adversarial design review (card #7059)

Date: 2026-11-27. Scope: criterion 1 of #7059 (gate before build). Read-only
review of live code; every "today" claim below cites a file and symbol read in
this session. Peer claims are tagged **[cited]** where they rest on primary
public documentation/papers, and **[unverified]** where this review could not
confirm them and the build must not rely on them.

## 0. What already exists (the baseline the card under-states)

The card is framed as if three things are new. Two of them partly ship already,
and the review must attack the real delta, not the pitch.

| Pillar | Live today | Source |
|---|---|---|
| String | 8-byte handle to the `data` word of a **104-byte heap block** (128-byte allocator class): `[count][data,len,cap][cache][64 inline bytes]`. Static literals are immortal (`cap == 0`, no count). Heap `slice`/`trim`/`before`/`after` **copy** (`rt_text_slice` → `rt_text_copy`). Sema-proven local bindings get a 40-byte **frame view** (`RT_TEXT_FRAME`, count `-1`), zero copy, whose `clone` copies. | `Compiler/JetRuntime/Source/Text.jet` (`RT_TEXT_BLOCK`, `RT_TEXT_INLINE`, `rt_text_slice`, `rt_text_immortal`, `rt_text_frame_view`, `jet_rt_string_clone`), `Compiler/JetSema/Source/Sema/Ownership/StringViews.jet` |
| RC | **One** count protocol for every heap block; always atomic (`LOCK XADD` via `sync_add` → `os_fetch_add_i64`), negative word = immortal, bit 62 = zero-at-free. Release has a sole-owner plain-store shortcut; retain does not. | `Compiler/JetRuntime/Source/Refcount.jet`, `Sync.jet` |
| `Shared<T>` | Direct field reads/writes already exist (`config.hits += 1`, one lock per statement); 104-byte cell with mutex, condvar, readers/writer words, weak count, replay id; `Shared.Weak<T>` with `downgrade()`/`upgrade()`; cycle check E0221 at the write. **No lock elision** — every access takes `sync_mutex_lock` twice (pin + acquire). | `Docs/Spec/spec.md` §"Shared state, local cells, and pools"; `Compiler/JetRuntime/Source/Shared.jet` (`shared_borrow_pin`, `shared_acquire_wait`, `jet_rt_native_shared_weak_upgrade`) |
| `@T` | Counted link block `[rc][excl state][payload size][payload]`, with **already split** local (plain store) and shared (atomic) variants plus dynamic read/write exclusivity. | `Compiler/JetRuntime/Source/Links.jet`, `Compiler/JetIR/Source/BuildLinks.jet` |
| `Pool<T>`/`ID<T>` | Lowered inline in IR: pool = `List<(generation: I64, value: T?)>`; `ID<T>` = `(index: I64, generation: I64)` = **16 bytes**; `add` **always pushes** (no slot reuse, every generation starts 0); `remove` leaves a `None` tombstone and bumps the generation; stale index stops. | `Compiler/JetIR/Source/BuildPools.jet` (`ir_pool_add`, `ir_pool_find`, `ir_pool_remove`) |

Consequences the card must absorb:

- "Immortal static literals = 0 RC ops" is **already true** (`rt_text_immortal`,
  `jet_rt_string_drop`). It is not a win to claim.
- `@T`'s local/shared split *is* the ST/MT dual-mode design the card wants for
  `Shared<T>`; the right move is to fold that mechanism into `Shared<T>` and
  delete `@T`, not to build a third RC protocol.
- Pool today leaks tombstones forever under churn and lets an `ID<T>` from pool A
  index pool B silently (both start at generation 0). These are live defects the
  "honest pool" work must fix, independent of syntax.

## 1. Prior art and strongest peers

- **Lean 4** [cited: Ullrich & de Moura, "Counting Immutable Beans", IFL 2019;
  `lean.h` `lean_inc_ref`/`lean_mark_mt`]. `m_rc > 0` single-threaded, `< 0`
  multi-threaded (atomic ops), `0` persistent. Objects reachable from a value
  handed to `Task.spawn`/a promise/an MT ref are marked MT by a graph walk
  **before** publication; the walk stops at already-MT nodes. Destructive
  update when `rc == 1` (reset/reuse). Lesson: sound only if *every* crossing
  calls the walk; one missed runtime crossing is a data race on a plain counter.
- **Swift** [cited: Swift `String` small-string representation, `_StringObject`,
  16-byte value, up to 15 UTF-8 bytes inline on 64-bit; `isKnownUniquelyReferenced`
  drives COW in stdlib collections]. Swift RC is atomic in shipped runtimes.
  The brief's premise that "Swift shipped per-object lazy biased RC and it had
  subtle races" is **[unverified]**: biased RC (Choi, Shull, Torrellas, PACT 2018)
  was prototyped *on* Swift, not shipped, and its cost centre was the slow-path
  merge queue and owner-thread exit. The compiler flag `-assume-single-threaded`
  (non-atomic RC everywhere) is unsound once any thread exists. Use the PACT
  paper as the citation, not a shipped-race story.
- **Rust** [cited: std docs]. `Arc<T>`/`Weak<T>` always atomic; `Rc<T>` non-atomic
  and `!Send`, chosen statically by type — zero runtime promotion, but two types.
  `Weak::upgrade` is a CAS loop that refuses at strong == 0. `String` = 24 bytes
  `(ptr, cap, len)`, no SSO; `&str` slicing is free and borrow-checked.
  `compact_str::CompactString` = 24 bytes, **24** inline bytes, using the fact
  that the last byte of valid UTF-8 is `< 0xC0` so values `0xC0..=0xFF` in the
  last byte discriminate inline length / heap / static. `slotmap` keys are
  8 bytes `(u32 index, u32 version)`, odd version = occupied, and
  `get_disjoint_mut([k1, k2])` returns `None` on duplicate keys;
  `slice::get_disjoint_mut` and `HashMap::get_disjoint_mut` are stable std APIs.
  `generational-arena` uses `(usize, u64)` indices.
- **Zig / Odin** [cited: Zig `std.heap.MemoryPool`, Odin `core:mem` pool
  allocators]. Pools are allocators, not handle maps; no generation check. They
  are the honesty reference (explicit allocator), not a safety reference.
- **Java 7u6** [cited: JDK-6924259 / Oracle 7u6 notes]: `String.substring`
  stopped sharing the parent `char[]` because small substrings pinned huge
  buffers. Direct precedent for the retention risk in Pillar 1.

## 2. Pillar 1 — COW/SSO zero-copy `String`

### 2.1 Proposed value layout (24 bytes, 8-aligned)

```
word0  word1  word2
[ data*  ][ len  ][ owner* | tag ]      heap / slice / static
[ 23 or 24 UTF-8 bytes ........ |len]   inline (last byte >= 0xC0 encodes kind+len)
```

- **Inline (SSO):** bytes 0..=22 plus last byte `0xC0 | len` for len <= 23, or
  CompactString's trick for exactly 24 bytes (last byte `< 0xC0` means "24-byte
  inline"). 24 is the honest maximum on 64-bit; the card's "<= 23" is fine as a
  floor.
- **Heap / slice:** `data` points *into* an RC'd byte buffer; `owner` is the
  buffer's count-prefixed block; `len` is the slice length. A slice is
  `(owner.bytes + start, end - start, owner)` + one retain. Zero byte copy.
- **Static:** `owner == 0` with the static tag → immortal, clone is a 24-byte
  copy, drop is nothing (today's `cap == 0` rule, re-encoded).
- **Frame view:** keep as a *compiler-internal* optimization (owner tag
  "borrowed, no RC") only where sema proves the owner outlives the local —
  today's `RT_TEXT_FRAME`. User-visible meaning is identical to a heap slice.

Alternative B (keep the 8-byte handle): tagged immediate for <= 7 bytes,
a separately allocated 40-byte slice header for slices. Smaller cutover, but
every non-tiny slice still allocates (~20–40 ns) and small strings above 7 bytes
still pay the 128-byte class. **Reject B** unless the cutover cost of A is
measured to be prohibitive; record the trade-off on the card.

### 2.2 Attacks

1. **ABI blast radius.** String moves from one word to three. 103
   `#Export(c)` text entries in `Compiler/JetRuntime/Source/` take/return it as
   one `I64`; 22 files in `Compiler/JetIR/Source/` use `ir_string_row`; every
   struct/enum/list stride containing `String`, `CoreLayoutABI.jet`,
   `CoreEnumABI.jet`, runtime routes (`IRRouteCarrier.Word/Handle` in
   `Compiler/JetIR/Source/RuntimeRoutes.jet`), the C ABI, web marshalling, and
   comptime value encoding change. This is the single largest item on the card
   and must be ranked first; it is not a "Part B" polish.
2. **Retention (Java 7u6).** `big.before("\n")` keeps a 1 GiB buffer alive.
   Required rule, executable and documented: a slice whose length fits inline
   is materialized inline (no owner retained); a longer slice shares. Peak
   memory must be measured in the peer cell on a "keep many short fields of
   large lines" program, not only on throughput.
3. **COW mutation and hidden O(n²).**
   - In-place `push` only when `rc == 1` **and** the slice ends at the buffer's
     used end **and** capacity suffices; otherwise copy into a fresh unique
     buffer with geometric growth (today's `rt_text_reserve_extra` doubling).
     After the first copy the string is unique, so a loop of pushes is
     amortized O(1).
   - The real O(n²) trap is `s = s + piece` (or `s.push`) where `s` is also
     retained elsewhere each iteration (`parts.push(s)` in the loop). That is
     inherent to value semantics; the requirement is a diagnostic or lint (with
     UI snapshot) when sema sees a clone of a growing string inside the loop
     that grows it — not a silent cliff. `a + b` where `a` is at last use must
     move `a` and append in place (`Ownership/LastUse.jet` already tracks last
     use); prove with an IR check that no copy is emitted.
   - Do not add a second builder type unless an existing one is retired
     (I8). Today a fresh builder is just a unique String (`rt_text_writable`);
     keep that: "StringBuilder" is not a new concept, it is `^s: ""` + push.
4. **Uniqueness under promotion.** `rc == 1` on an MT block read with a plain
   load is still sound for the writer (its own reference keeps it >= 1 and only
   it can clone) — same argument as `jet_rt_rc_unique` today. Keep it.
5. **Cache word.** Today `+24` caches ASCII prefix / scalar count per heap
   block. Under slicing the cache describes the *buffer*, not the slice. Either
   drop it or key it to `(data, len) == whole buffer`; a stale cache on a slice
   is a correctness bug in `len_chars`/indexing.
6. **C FFI.** A slice is not NUL-terminated and its `data` is not the buffer
   start. `Docs/Spec/spec.md` (C ABI, "A C `String` return must be non-null,
   NUL-terminated…") only covers returns; argument passing must copy to a
   NUL-terminated temporary unless the slice ends at the buffer end with a spare
   NUL byte. Pointer stability: inline strings live in the value, so passing
   `&s` bytes to C across a move is a dangling pointer — FFI must always pass a
   heap or copied buffer.
7. **I9 parity.** O0/comptime/O1/O2/web must share the layout and the slicing
   rule (inline-vs-share threshold) or `strong_count`/peak-memory reports
   diverge. Web: a single-threaded wasm/JS host makes every count ST; if web
   ever gains threads the promotion walk applies identically.
8. **`View<str>` and `from` boilerplate (criterion 1(c)).** With owned zero-copy
   slices, `-> View<str> from line` (e.g. `Examples/Memory/Views/ReturnedViews.jet`,
   `ViewFromParam.jet`) becomes redundant for strings: `-> String` costs one
   retain. Delete `View<str>` and E2307's string branch; keep `View<T>` /
   `ViewMut<T>` for `[T]` windows and in-place edits, where a borrowed window
   still has no RC cost and `ViewMut` exclusivity has no COW substitute. This is
   a user-facing removal and needs a ballot; the cost is that a returned string
   slice now pays 1 RC inc/dec where Rust `&str` pays 0 — acceptable only if the
   peer cell proves `Jet/Rust < 1.00` anyway.

## 3. RC mode: single-thread non-atomic with transitive promotion

### 3.1 Encoding (one protocol for String, List buffers, boxes, Shared cells)

Extend `Refcount.jet`, do not fork it: negative word stays immortal; bit 62
stays zero-at-free; **bit 61 = MT**. Retain: one load, `word < 0` → return;
`word & MT` → `LOCK XADD`; else plain store. Release mirrors it. Predicted
branch ≈ 1 cycle; plain add vs `LOCK XADD` (≈ 18–20 cycles uncontended on
modern x86) is the whole win. `Links.jet`'s local/shared split collapses into
this bit.

### 3.2 Soundness attacks

1. **Invariant:** *an ST object is reachable (strongly or weakly) only from one
   thread.* Every way a reference becomes visible to a second thread must run
   `mark_mt(root)` before publication. `Ownership/Crossings.jet`
   (`SemaSendCrossing`) enumerates the language crossings: TaskCapture,
   TaskResult, ChannelSend, ParallelWorker, HTTPHandler, Kernel,
   InterruptCallback, SharedStorage, FrozenSnapshot. Each needs a runtime mark
   at its lowering site. Missing any one is an I1 violation (use-after-free via
   lost increment).
2. **Runtime-internal crossings** are the likeliest miss: stdout/stderr buffered
   writers, async I/O completions, scheduler work stealing of fibers that hold
   ST references, `jet perf`/record-replay threads, static cells
   (`Ownership/StaticCells.jet`). **Fiber migration**: if the scheduler can
   resume a fiber on another OS thread, "ST" must mean *one task*, not one OS
   thread, and the scheduler's handoff must be a release/acquire edge. The
   build must list every runtime entry that moves a value or a fiber across
   threads and prove each marks.
3. **Store into an MT container.** Writing an ST value into an MT `Shared<T>`
   cell, MT list, or MT map must mark it at the store (Lean's
   `lean_st_ref_set` behaviour). Today's `Shared` write paths
   (`jet_rt_native_shared_edit`, `try_replace`, `#Transact` commit) all need it.
4. **Weak upgrade race during promotion (owner requirement).** Hazard: MT cell
   X holds `Weak<Y>` where Y is ST; thread B reads X, upgrades → B owns a strong
   ref to ST Y while A does plain increments → lost update. Rules required:
   (a) `mark_mt` traverses weak edges to the target cell header (and, because an
   upgrade yields the payload, the payload); (b) storing a `Weak` into an MT
   container marks its target; (c) `downgrade`/weak count uses the same mode
   bit as the strong count; (d) `upgrade` on an MT cell stays the existing CAS
   that refuses at zero (`shared_count_retain`). Promotion itself runs on the
   only thread that can reach the ST graph, so the walk races nothing; the
   publication store after it must be a release (x86 TSO gives it; wasm/ARM
   need an explicit fence).
5. **Walk cost.** O(newly promoted nodes); stops at MT/immortal. Adversarial
   case: sending a 10 M-node ST graph once costs a 10 M-node walk on the
   sender. Must appear in `jet perf` attribution and in a paired benchmark
   (send-heavy channel program) against Rust `Arc` and Go.
6. **Never demote.** Once MT, a block stays atomic. Long-lived servers trend to
   all-MT; measure the steady-state share.
7. **Cycles.** Unchanged: E0221 strong-cycle check at the write, weak back-edges.
   Promotion does not create or break cycles. Leak report on cycle via closures
   must stay as today.

## 4. Pillar 2 — direct-field `Shared<T>` / `Weak<T>` with lock elision; retiring `@T`

### 4.1 What is new

Direct fields already ship. New: (i) lock elision on ST cells, (ii) `?.` through
`Weak<T>` and optional `Shared<T>` fields (`bob.parent?.hp -= 1`), (iii) `@T`
deletion.

### 4.2 Attacks

1. **Fast path budget.** Today a field read = `shared_borrow_pin` (mutex lock,
   CAS, `sync_add`, unlock) + `shared_acquire_wait` (lock, loads, unlock) +
   release (lock, notify_all, unlock) + unpin (lock, add, unlock) + replay
   ticket. The ~1 ns target requires the ST path to skip **all** of that: one
   mode-bit test, a plain exclusivity word (as `jet_rt_link_read_enter_local`),
   the access, the plain clear. Record/replay (`replay_take`) and wait-cycle
   bookkeeping must also be elided on ST cells, or `jet run --record` changes
   the cost profile; justify that replay needs no events for a cell no other
   thread can see.
2. **Re-entrancy / aliasing (criterion 1(a)).** Today recursive acquisition
   *blocks* and the wait-cycle detector stops the task. The elided path must
   produce the **same failure and report** (I9 across tiers and across ST/MT
   modes): e.g. `a.x = f(a)` where `f` edits `a`. Use the Links.jet dynamic
   exclusivity word and raise the same stop the MT path's cycle stop raises.
   A behaviour that differs by whether a cell happened to be promoted is a
   defect.
3. **Promotion while an elided guard is held.** `guard_edit()` then
   `task { … cell … }` in the same scope promotes a cell whose ST guard is
   live. The promotion must convert the plain exclusivity word into the real
   writer word (owned by the current task) so the spawned task blocks rather
   than racing; release then takes the MT path because it re-reads the mode bit.
   Proof: a test that spawns under a held guard on every tier.
4. **Lock ordering.** Per-statement multi-cell writes use the existing ordered
   engine / `#Transact` stable-address order. Mixed ST/MT in one statement:
   ST cells need no ordering (no other thread), MT cells keep address order.
   Must be stated and tested (two-cell statement, one cell promoted mid-run).
5. **`?.` through `Weak`.** `bob.parent?.hp -= 1` = upgrade (retain), acquire,
   RMW, release, drop (release). On ST that is ~4 plain ops; on MT two atomics
   plus the lock. The statement is atomic per cell, but the upgrade and the
   write are not one step against a concurrent last-owner drop — fine, because
   the upgraded strong ref keeps the payload alive. `?.` on stale must be a
   no-op with the same meaning on every tier; a write through `?.` that is
   silently skipped must be visible in `jet inspect`/lint or it becomes a
   silent-logic-bug factory. Decide on a ballot whether `?.=` on a stale weak
   is silent or a `must_use`-style warning.
6. **`@T` retirement and "freeing `@`".** `Spellings.jet` row for `@` reads
   "live link; …"; `Markers.jet` line 12 notes infix `@` remains the
   package-source reference (`c@system`). So only **prefix** `@` is freed; the
   card's "freeing @" overstates. Any future reuse of prefix `@` needs its own
   ratification; the build must change the row to "retired live link" with a
   migration diagnostic, not delete it silently.
7. **Semantics gap `@T` → `Shared<T>`.** `@T` local links had no mutex,
   no weak count, and a 24-byte header; `Shared<T>` cells are 104 bytes. Memory
   per node for linked structures (`Examples/Memory/LiveLinks.jet`,
   `Tests/Cases/MemorySemantics/RecursiveEnumEdgesAreOneAllocation`) will grow
   ~4× unless the ST cell layout is slimmed (mutex/condvar/replay words
   allocated lazily at promotion). Required: a compact ST cell (count, excl,
   weak, drop fn, payload) that grows its sync block on promotion, or a
   measured justification that the peer memory floor still holds.

## 5. Pillar 3 — honest `Pool<T>` / `ID<T>`

### 5.1 Layout

`ID<T>`: 8 bytes, `Copy`, `(index: U32, generation: U32)`, type-tagged by `T`
in sema (already: `Literals.jet` E0112 "`Pool` indexes need a matching
`ID<T>`"). Slot: `(generation: U32, value: T?)` with a free list threaded
through vacant slots (slotmap's odd/even occupancy bit saves the `Option` tag).
This halves today's 16-byte ID and adds slot reuse, which today's
`ir_pool_add` lacks.

### 5.2 Attacks

1. **Generation wraparound.** With reuse, a slot reused 2³² times aliases a
   stale ID. Rule: when a slot's generation would wrap, retire the slot
   permanently (never pushed back on the free list). Cost: ≤ 1 leaked slot per
   4 G removals of the same slot. Deterministic, documented, tested with an
   injected near-wrap generation.
2. **Cross-pool misuse (live defect).** Today both pools start every slot at
   generation 0, so `a_id` indexes `pool_b` and silently returns another
   entity. Memory-safe (it is a live `T`) but a logic hole the card calls
   "honest". Options for the ballot:
   (a) per-pool generation salt (random/counter start per pool) — 8-byte ID,
   probabilistic detection (~2⁻³²);
   (b) 16-byte ID with a 32-bit pool identity — deterministic, doubles ID size
   and loses the 8-byte claim;
   (c) accept and document, as slotmap does. Recommend (a) as default with a
   debug-and-release-identical check (I9), and put (a)/(b)/(c) on a ballot.
3. **Disjoint multi-element edits.** `edit_pair`/`edit_many` are new public
   API. I8 (1-in-1-out) and orthogonality: lists and maps need the same thing
   (Rust ships `get_disjoint_mut` for slices, maps and slotmap). Do **not** add a
   Pool-only method. Ballot one mechanism that works for every indexable owner
   (e.g. a disjoint-places form whose dynamic indices get a runtime
   distinctness check, extending the existing "dynamic projections conservatively
   overlap" rule in spec §"Named views and places"), returning `ViewMut<T>`
   windows; duplicate IDs → `None`/stop with the same message on every tier;
   stale IDs → same as `pool[id]`. Zero `#Unsafe` in Core: the split is a
   compiler-lowered structural split, as constant-index splits are today.
4. **`?.` on stale (`people[bob]?.hp -= 1`).** Today `pool[id]` on stale *stops*
   (`ir_pool_slot_address`). Adding an optional index form makes two meanings
   for one place (stop vs skip). Pick one surface per meaning: `pool[id]` stops,
   `pool[id]?` (or `pool.get(id)`) is optional — ballot the spelling against
   `Spellings.jet`.
5. **Name collision.** `core.mem.Pool` (slot allocator, no type argument,
   `Core/Mem/Mem.jet` line 85) and the generic `Pool<T>` both map to runtime
   handle `JetPool` (`Compiler/JetFoundation/Source/Registry/RuntimeHandles.jet`
   rows for `<corelib>/core.mem.Pool` and `Pool`). Arity disambiguates today,
   but users and diagnostics see one name for two concepts. Rename the
   allocator (ballot; e.g. `Slots`) and split the runtime handle names.
6. **"Zero ambient pool guessing".** Correct direction: `people[bob]` names the
   pool explicitly. The open `#7031` design (pool-ID navigation without naming
   the pool) must be closed as superseded by ballot, not left as a competing
   mechanism (I8).

## 6. Performance obligations (peer floor, `Jet/peer < 1.00`, wall and peak memory)

Paired two-program cells, each against Rust (and Go/Swift where matched):

1. Line-split/trim/field-extract over a 100 MB file (Rust `&str` and
   `String`/`CompactString` variants): wall + peak RSS.
2. Many-small-strings (map of 10 M short keys): peak memory — today's 128-byte
   block per string is a likely current loss vs Rust; SSO must win it.
3. Retention adversary: keep short slices of large lines (§2.2.2).
4. Shared field RMW in a hot ST loop vs Rust `Rc<RefCell<T>>`, and MT vs
   `Arc<Mutex<T>>`.
5. Send-heavy channel of graphs (promotion walk cost) vs Rust `Arc`.
6. Pool churn (add/remove 100 M) vs `slotmap` — wall + peak (proves slot reuse).

IR/asm checks at O2: ST retain/release inline to load+test+add, no `lock`
prefix; slice creation contains no call to an allocator for inline-sized
results; `a + b` at last use emits no copy; `pool[id]` is index+compare+branch
with no call; elided Shared field read contains no `sync_mutex_lock`.

## 7. Verdict

**Conditional-Go.** The direction is sound and matches strongest prior art
(Lean 4 RC modes, CompactString layout, slotmap IDs). Build may start only after:

1. Ballots: (i) delete user-facing `View<str>`/string `from` returns;
   (ii) prefix-`@` retirement wording; (iii) cross-pool policy (a/b/c);
   (iv) one disjoint-edit mechanism across List/Map/Pool (not Pool-only);
   (v) stale-optional index spelling; (vi) allocator `Pool` rename;
   (vii) silent vs warned `?.` write on stale weak; (viii) supersede `#7031`.
2. Card criteria added (owner may reword): the crossing inventory of §3.2.1–3.2.2
   with a per-site mark proof; the Weak rules of §3.2.4; compact ST cell for
   `Shared<T>` (§4.2.7); retention rule (§2.2.2); cache-word rule (§2.2.5);
   FFI NUL rule (§2.2.6); the six peer cells and IR/asm checks of §6.
3. Ranking for the build (largest gap first): String 24-byte ABI + SSO (fixes
   the 128-byte-per-string memory loss and every slice copy) → RC mode bit +
   promotion → Shared ST elision + `@T` deletion → Pool reuse/8-byte ID.

## 8. Binding cutover list

Runtime (`Compiler/JetRuntime/Source/`):
- `Refcount.jet` — MT bit, ST plain paths, `mark_mt` walk entry.
- `Text.jet`, `TextMethods.jet`, `TextBytes.jet`, `TextFmt.jet`, `TextGlue.jet`,
  `TextRoutes.jet`, `Display.jet`, `Unicode*.jet`, and every file exporting the
  103 text entries — 24-byte carrier; delete `RT_TEXT_BLOCK`/`RT_TEXT_INLINE`
  block layout; `rt_text_slice` shares instead of copying.
- `Shared.jet`, `SharedTransactions.jet` — ST fast path, lazy sync block, mark at
  store, weak rules.
- `Links.jet` — delete.
- `MemStorage.jet` — split allocator `Pool` naming.
- Every runtime thread/fiber handoff site found by the §3.2.2 inventory.

IR / backend:
- `Compiler/JetIR/Source/BuildLinks.jet` — delete; remove link refs in
  `Build.jet`, `BuildExpr.jet`, `BuildOwn.jet`, `BuildTypes.jet`,
  `BuildChannels.jet`; `EntryOwned.jet` link rows.
- `Compiler/JetIR/Source/BuildPools.jet` — 8-byte ID, free list, wrap retire,
  salt, disjoint split.
- `RuntimeRoutes.jet` string carriers; the 22 `ir_string_row` users;
  crossing-site `mark_mt` emission.
- `Compiler/JetBackend/Source/CoreLayoutABI.jet`, `CoreEnumABI.jet`, `X64/`
  inline clone/drop sequences.

Sema / foundation / parser / CLI:
- `Compiler/JetSema/Source/Sema/Links.jet` — delete; `Expressions/Places.jet`
  link paths.
- `Ownership/StringViews.jet`, `Statements/Returns.jet`, `Statements/Bindings.jet`,
  `Scopes.jet`, `Calls/Direct.jet`, `Expressions/Literals.jet` — string-view /
  `View<str>` paths per ballot (i).
- `Ownership/Crossings.jet` — crossing list is the source for mark sites.
- `Calls/SharedOwner.jet` — Pool methods, disjoint edit per ballot (iv).
- `Compiler/JetFoundation/Data/Syntax/Spellings.jet` (`@` row),
  `Data/Syntax/Markers.jet` comment, `Source/Registry/RuntimeCExports.jet`
  (link exports), `Source/Registry/RuntimeHandles.jet` (`JetPool` split),
  `Source/Registry/DiagnosticRows.jet`, `Compiler/Prelude/Diagnostics.jet`
  (link codes, E2307 text, new migration codes).
- `Compiler/JetParser/Source/Formatter/Expressions.jet`,
  `Compiler/JetParser/Tests/Entry.jet`, `Compiler/JetCLI/Source/CLI/InspectB.jet`,
  `Compiler/JetCLI/Source/CLI/SyntaxDictionaryData.jet`,
  `Compiler/JetCLI/hot-text.list`, `Compiler/JetCLI/Tests/FmtLossless.jet`
  (`View<str> from` cases).

Core: `Core/Mem/Mem.jet` (allocator rename, comment at lines 38–43);
`Core/UI/Host/DragDrop.jet` (link mention).

Examples (with goldens): `Examples/Memory/LiveLinks.jet` → `Shared`/`Weak`
rewrite; `Examples/Memory/README.md`; `Examples/Memory/Views/StringView.jet`,
`ReturnedViews.jet`, `ViewFromParam.jet`; `Examples/Memory/EntityWorld.jet`,
`EntityTree.jet`; new examples for promotion, weak `?.`, disjoint pool edit,
retention rule.

Tests: `Tests/CLI/explain_syntax_at.txt`;
`Tests/Guards/SigilMeaningTable/sigil_meaning_table.jet`;
`Tests/Cases/MemorySemantics/RecursiveEnumEdgesAreOneAllocation.fixture`;
`Tests/Cases/BuildEntry/CompilerEventOneEnum.fixture`,
`StagesSequenceThreadIndependent.fixture`;
`Tests/Cases/CoreQualifiedCalls/AReexportedCoreEnumIsOneTypeThroughEachModule.fixture`;
`Tests/Cases/RatifiedP20/DKeyTraits1Contract.fixture`;
`Tests/Cases/DevPartsRoutine/ReturnedStringViewFieldMatchesAllExecutionTiers.fixture`;
`Tests/Cases/Ownership/ZeroCopyParserBlocksSourceReplacementWhileTokenIsLive`,
`DeclaredViewCallbackFromFreezesOnlyNamedSource`;
`Tests/Cases/FmtRoundTripRegressions/` view-provenance cases;
`Tests/Cases/SharedTransaction/SharedCompoundPlacesMatchO0O1O2.fixture`;
UI snapshots for every new/changed code.

Docs: `Docs/Spec/spec.md` (§"Named views and places" link paragraph,
§"Zero-copy string views", §"Shared state, local cells, and pools", C ABI
string passing); `Docs/Spec/syntax-decisions.md` (`@` entry); Reference pages
for String/Shared/Pool. `Tools/Agent/ScriptPairing.jet` (link mention).

Sweep before landing the stricter rules: every `.jet` in the repo for prefix
`@T`, `View<str>`, `-> … from` on string returns, and `core.mem.Pool` uses.

## 9. Required proofs

- Per-tier goldens (O0 `jet run`, comptime, O1, O2 `jet build`, web) for each
  pillar's example, showing the feature's own output (counts, `strong_count`,
  stale/duplicate messages), not "ok".
- Concurrency stress on O2 under a race detector-equivalent run: promotion via
  each `SemaSendCrossing` kind; weak upgrade from a second task during sender
  promotion; spawn under a held elided guard.
- The six peer cells of §6 with `Jet/peer < 1.00` wall and peak memory, plus a
  second, different program per root fix.
- O2 IR/asm checks of §6.
- Full collect-all suite on the batch compiler before closing (cross-cutting:
  runtime, codegen, sema, Core).
