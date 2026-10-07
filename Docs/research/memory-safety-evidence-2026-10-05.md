# Memory-safety evidence: borrow-checker corpus, copy audit, safety net (2026-10-05)

This note is dated evidence for the memory-model work. It records three
results from 2026-10-05:

- an acceptance corpus that compares Jet's checker with Rust's borrow checker;
- an audit of where Jet copies, retains, locks or checks where Rust would
  borrow or move;
- the design of SafetyNet, a second independent safety proof in MIR Lint.

It owns no plan or status. The design that uses this evidence is the proposal
[memory-model-2026-10-05](../proposals/memory-model-2026-10-05.md). Work lives
on these Tower cards:

| Card | Holds |
|---|---|
| #4620 | Memory model: Rust-or-better safety, easier than Rust, never copy where Rust borrows or moves |
| #4619 | Safety net after Rust removal: MIR Lint proves exclusivity, view validity and sendability independently of sema |
| #4621 | Sema accepts collection mutation laundered through a local closure; closures that push into a captured list ICE |
| #4638 | I1: JetArena retains map values as exact-Int unless marked raw |
| #4639 | I1: `Examples/features/memory/pin.jet` segfaults inside JIT code |

Scratch record (not committed): `/mnt/jetscratch/scratch/memcorpus/`
(`RESULTS.md`, `AUDIT.md`, `HANDOFF.md`, `cases/`, `out/`, `results.tsv`,
`emit-cases.tsv`), and `~/.cache/jet-dev/scratch/SafetyNet/` (`HANDOFF.md`,
`PLAN-3-4.md`, `proto.mjs`). The SafetyNet code is on branch
`safety/mir-lint` at `507cf8ef3`.

## 1. Acceptance corpus: Jet versus Rust NLL and Polonius

### Method

The corpus has 87 small Jet programs. Each one mirrors a Rust program whose
verdict is known, and the file header carries the Rust original and its
verdict. The shapes come from:

- the rustc NLL RFC 2094 problem cases #2, #3 and #4;
- two-phase borrows;
- Polonius issues #46859 and #47680;
- `split_at_mut` and `get_disjoint_mut`, and the map Entry API;
- iterator invalidation, lending iterators (GATs), and struct-held borrows;
- self-reference and parent links;
- closure captures, `std::thread::scope`, and `Send`/`'static`.

`run.sh` ran `jet check` with release binary
`/mnt/jetscratch/candidates/dev-05ea86f65/jet` (dev `05ea86f65`). Every program
that `jet check` accepted was also lowered with `jet emit --rust`, which
catches accepts that cannot actually build.

| Mark | Meaning |
|---|---|
| = | Jet agrees with Rust |
| FR | False reject: Rust (NLL or Polonius) accepts and Jet rejects. A usability gap. |
| FA | False accept: Rust rejects and `jet check` accepts with no runtime guard. A possible safety hole. |
| +P | Jet accepts a Rust reject soundly because it proves more, for example disjoint constant indexes |
| +C | Jet accepts a Rust reject by copying (value semantics). Sound, but it costs a copy. |
| +D | Jet accepts a Rust reject through a dynamic cell (Rc/RefCell boxing). Sound, with a runtime cost. |
| ICE | `jet check` accepts, but lowering dies with an internal compiler error |

### Totals

Rust NLL accepts 49 programs and rejects 38. Polonius also accepts four of the
NLL rejects (14, 48, 48b, 49), which gives 53 accepts and 34 rejects.

| Outcome | Count | Cases |
|---|---|---|
| Agree | 59 | Includes 49 (a Polonius-only accept), 37 (Jet infers Rust's `move`), six Jet idiom variants and four Jet-only controls |
| False reject | 20 | 06, 14, 14b, 15, 16, 17, 20, 24b, 24c, 32, 32b, 33, 39, 40, 48, 48b, 53, 54, 68, 65 |
| Static false accept | 2 | 36, 62. MIR legality verification stops both. |
| Sound extra accept | 6 | +P 11, 40b; +C 46, 56, 64b; +D 66 |
| Check accepts, lowering ICEs | 4 | 32b, 36, 62, 65 (already counted above) |

Of the 53 programs Polonius accepts, Jet accepts and builds 33. The other 20
are false rejects or ICEs.

Every Rust rejection that guards memory safety is also a Jet rejection, except
the closure case in section 2. That includes:

- a view held across a push (03, E0212);
- a write inside the call that reads the same value (05, 58, E0204);
- iterator invalidation (18, E0507);
- returning a view of a local (26, E2305);
- a self-referential struct (30, E2307);
- use after move (45, 67b, E0121);
- borrows crossing into tasks (41, 42, 43, 60; E1101, E1102);
- arena reset while a view is live (55, E0632).

### False rejects

These are ranked by how common the Rust pattern is.

| # | Gap | Cases | Jet today |
|---|---|---|---|
| 1 | Method call through a write window | 06, 08; probe p4 | `a :: &s.xs; &a.push(x)` reports the window conflicting with itself (E0212 + E0202). Field assignment through a window works. |
| 2 | No mutable map lookup or Entry | 14b, 15, 16, 17 | A map value cannot be returned as `ViewMut`, a `map.get` match payload is not a place, `m[k] += 1` is E0164, and `setdefault` returns by value. Workarounds cost 2–3 lookups plus a clone. |
| 3 | No mutable iteration | 20 | `loop p in &ps` gives a read-only `p` (E0205). The only spelling is an index loop with a bounds check on every access. |
| 4 | `@` links not shipped | 32, 33, 48 | D-MEMREF1 is ratified, but `@p` is E0107 and `@Node` is E0119. `Shared.Weak` passes check and ICEs on emit (32b). |
| 5 | Polonius conditional return | 14, 48, 48b | No write window into an enum or option payload (E0202). |
| 6 | Two slice types | 39 | A bound window is `View<Int>`, and a `[Int]` read parameter rejects it (E0112). An unbound `v[0..1]` argument is accepted but copied. |
| 7 | Split windows into tasks | 40 | `split_write` halves cannot enter task bodies (E1102, E0111). Constant place windows can (40b). |
| 8 | No swap, take or replace through `&` | 53, 54 | Moving out of a write place is E0201. The workaround is two deep copies. |
| 9 | Local borrowing structs | 24b, 24c | A `View<str>` field fills from a parameter view in a helper but not from a local view (E2307), and `View<str>` has no `after`, `before` or `trim` (E0311). |
| 10 | Wrong loop diagnostic on a task move | 68 | `tx.send(^buf)` inside `task ^tx { … }` reports "moved on an earlier loop iteration", but there is no loop. |

On top of these, every closure that pushes into a captured list ICEs on
lowering (65), so the "closure mutates a collection" pattern cannot build at
all.

`View<T>` provenance already covers several shapes:

- returning borrows of parameters (27, 28);
- struct fields that hold borrows built in a helper (24);
- lending `next(&self) -> ViewMut from self` (63, 63b);
- Polonius's early return of a shared borrow (49);
- arena invalidation (55).

It does not yet cover borrows of map values, enum or option payload places,
mutable iteration, reassignable `&mut` cursors, or building borrowing structs
locally. A window into one element is a one-element slice (`pair[0][0]`);
Rust's `&T` to a single element has no direct analogue.

## 2. The closure false accept (#4621)

Two programs pass `jet check` although Rust rejects them:

- **62, loop with a closure push.** `grow :: (x: Int) -> { &v.push(x) }` followed
  by `loop x in v { grow(x) }` is accepted, while the direct form (a push
  inside the loop) is E0507.
- **36, owner read while a capturing closure is live.** The owner is read while
  a closure that holds `&` access to it is still live.

Root cause: a local closure that writes a `:=` list does not hold a live write
loan against later reads or loops of that list. A separate closure-capture
lowering defect makes these accepts fail closed for now. MIR legality
verification rejects the closure ("mismatched value at capture slot 0"), so no
binary or eval can run. The same defect also blocks the Rust-accepted control
65, so the defect alone cannot tell safe programs from unsafe ones.

Scalars are different. A closure that writes a captured scalar (66) compiles
through `Rc<RefCell<Option<T>>>`, so the worst case there is a RefCell panic,
not undefined behavior.

Why it matters: today the backstop is the backend's verifier, and behind it
rustc. Once the native backend replaces the Rust path and the lowering defect
is fixed, nothing would catch this sema miss, and it would become silent
memory corruption. That makes it the concrete witness for #4619. #4621's
criteria ask sema to treat a `&`-capturing closure as a live write borrow of
each captured place for the closure's whole live range, to make case 65 build
and run on every tier, and to have MIR Lint re-check captures. The proposal's
card draft K9 is the same fix.

## 3. Copy audit: where Jet pays and Rust does not

The audit used the same binary, over `git archive 05ea86f65 Examples Core`.
It used two instruments:

1. `jet audit copies --json` on all 1,370 `.jet` files. 1,057 were audited;
   the other 313 fail `jet check` or are not entry files.
2. `jet emit --rust` on every file, counting patterns in the lowered user and
   Core code. 848 programs emit; Core code lowered into a program counts once
   per program.

Costs are **estimates**. The paired timing job `MemEvidence-copybench` (12 Jet
release builds plus Rust references, `bench/`) ran and failed with rc 1. Every
build stopped on `jet-env: CARGO_TARGET_DIR=/mnt/jetscratch/targets/MemEvidence
is not checkout-owned`, and the timing script also lacked `bc`.
`bench/times.tsv` has no measurements, so this note claims no measured ratio.

### Implicit copies reported by `jet audit copies`

There are 15,919 reports at 1,539 unique sites (1,303 in Core, 236 in
Examples) across 463 programs. Every report says `size=dynamic kind=implicit`,
without the type.

| Rust would | Sites | Example |
|---|---|---|
| move: a read parameter stored into an owning slot | 791 | `Core/http/http.jet:247` `&fields.push(Header{name: name, …})` |
| move: last use of a local | 174 | `Core/files/files.jet:665` `WalkEntry{path: e.path, …}` |
| move: temporary or complex | 19 | `Core/archive/archive.jet:218` `Ok(files[0].data)` |
| clone too (source used later) | 236 | `Core/web/store.jet:136` `cursor :: store.cursor` |
| no copy: operator operand or comparison | 175 | `Core/text/text.jet:857` `is_ri(cur)` (Int) |
| no copy: unit enum variant or constant | 101 | `Core/encoding/json.jet:1272` `value := DataTree.Null` |
| borrow: loop over a field | 43 | `Core/crypto/crypto.jet:381` |

A hand-typed random sample of 60 non-operator, non-enum sites found 44 heap
values, 13 scalars and 3 unclear. That puts about 920 of the 1,539 sites as
real heap copies. The tool over-reports (scalars, `==` operands) and also
under-reports: none of the copies in the emitted-Rust table below show up in
it (cases 21, 38, 39c, 56, 64b and 67 report 0).

Core also passes 604 explicit `~x` copies as call arguments (219 in
Examples). Most go into read parameters, which already borrow for free, for
example `files.basename(~path.raw)` and `u32le(~key, 0)` twelve times per
ChaCha block.

### Copies, retains, locks and checks in the emitted Rust

The 848 emitted programs have about 65,600 classified sites.

| Class | Pattern | Sites | Rust equivalent |
|---|---|---|---|
| retain | `Int` is a tagged word, not `Copy`: `.clone()` on every read, with an atomic retain when a bigint has spilled | 45,694 | `i64` register copy |
| copy | String literal built with `String::new(); push_str(…)`, even into read parameters and `==` | 7,600 | `&'static str` |
| runtime check | `loop x in list` lowers to index plus a bounds check per element | 5,444 | `for x in &v` |
| copy | Heap `.clone()` of lists, strings, structs and match payloads, including `.lazy()` sources | 4,436 | borrow or move |
| RC / dynamic | Function value is `Rc<RefCell<Option<Box<dyn FnMut>>>>`, and each call does 2 `borrow_mut` plus a dynamic call | 1,404 | `impl Fn`, inlined |
| copy | `print` or interpolation argument cloned | 84 | `Display` by reference |
| copy | `map.get(k)` is `get(k).cloned()` | 67 | `&V` |
| copy | Loop source cloned when the body also reads the list | 65 | `for a in &names` |
| copy | `task ^x` and escaping closures clone the capture instead of moving it | 30 | `move` |
| copy | Adapter receiver consumed: `xs.fold(…)` on a read parameter clones the list | 25 | `xs.iter().fold` |
| copy | View materialization (`jet_view_copy`) | 21 | borrow |
| lock | `shared` field access takes a permit and an atomic revision per statement, plus a fresh boxed closure | 34 | `Rc<RefCell>` in one task |
| RC | Captured `:=` local boxed as `Rc<RefCell<Option<T>>>` | 5 | `&mut` capture |

The corpus shows three costs that a static count cannot:

- share-on-reuse (D-COPY-DEFAULT1) is an eager `Vec<String>.clone()`, not
  copy-on-write (46);
- swap and take need two deep copies (53b, 54b);
- comparisons can clone: `window[0] == s` clones the element (64b).

### What follows for the model

| Jet mechanism today | Rust would | Gap |
|---|---|---|
| Implicit view to owned copy | move (last use, kept parameter) or clone | About 920 heap copies. Inferring "keeps parameter → `^T`" is missing. |
| Explicit `~` into read parameters | borrow | 604 Core sites; a lint would catch them |
| Closure as Rc + RefCell + `Box<dyn>` | `impl Fn` or `&mut` capture | An allocation and a dynamic borrow per call, per element inside adapters |
| `Int` as a counted bigint word | `i64` Copy | A retain branch on every read, and a `Drop` on every slot |
| `shared` always locks | `Rc<RefCell>` unless sent | No local form (D-MEMREF-TASK1 is unbuilt) |
| List loop as index + check, sometimes a clone | `for x in &v` | A check per element, plus a clone when the body reads the list |
| `map.get` returns a clone | `&V`, `get_mut`, `entry` | A clone plus extra lookups |
| `task ^x` clones | move | O(n) copy (case 67) |

#4620 logs a follow-up from CopyCut. Removing the 791 read-parameter copies
needs either callee "keeps-parameter" inference or a contract change. Both
conflict with the ratified rule that sema never raises a parameter's access
from how the body uses it, so it is an owner choice.

## 4. SafetyNet: an independent second proof in MIR Lint (#4619)

### Why

Today rustc's borrow checker and `Send`/`Sync` re-check the Rust that Jet
generates, and a rustc rejection is an ICE (I2). Case 62 shows this backstop
catching a real sema miss. The native backend has no such check. MIR Lint
(`Docs/spec/mir-lint.md`) already re-checks types, `ownership.move` and effect
rows, but it had no exclusivity, view-validity or sendability rules. #4619
gates Rust removal (D-EXEC2) on criteria 1–4.

### Design (branch `safety/mir-lint`, WIP commit `507cf8ef3` on dev `3f64b4fb6`)

| Rule family | Proves |
|---|---|
| `alias.exclusive` | A write place is unique, and no live read overlaps a live write. Includes an in-call check that no argument overlaps a write argument. |
| `view.valid` | No owner is moved, resized or written while a view into it is live, and no view outlives its owner set. Returned views are checked at terminators. |
| `send.boundary` | Every value that crosses a task, channel or parallel boundary is copied, moved, frozen or `Shared`; no `Cell` or view crosses. |

- The rules live in `Compiler/JetOptimizer/Source/Verification/Lint.jet` as
  `MIRLintRule.AliasExclusive`, `ViewValid` and `SendBoundary`.
- The lint index gains, per signature, `parallel_routes`, `channel_sends`,
  `task_joins` and `captures`.
- The safety bits ride the existing ownership dataflow after the ownership bits
  (`MIRLintOwnership.safety`, `mir_lint_safety_step` inside
  `mir_lint_transfer`), so they get guard pruning for free and the lint stays
  linear in program size. A failure is an ICE, never a user diagnostic.
- Mutation-proof fixtures in `Compiler/JetOptimizer/Tests/LintFixtures.jet`
  (about 24) include a clean and a planted program for each rule. They also
  include programs that sema rejects with E0204, E0212, E0121, E1101 (×3),
  E1102 (×3) and E2305, plus #4621 cases 36 and 62. MIR Lint must reject each
  one.
- On the branch, `Docs/spec/mir-lint.md` gains the family rows and a "Safety
  families" section, and `Docs/spec/safety.md` gains a post-Rust safety
  argument.

### False-positive evidence

A JavaScript prototype (`proto.mjs`) mirrors the rules over the Rust
`JET_DUMP_MIR` corpus of about 850 goldens (BackendO11's
`g13-preintegration-partial`).

- Run 3 found 8 false `alias.call` hits, all in `Core/archive`
  `inflate_fixed`/`inflate_dynamic` (`&out.push(out[i])` with U8 elements).
  The fix treats scalar-ABI values as copies, in both `Lint.jet` and the
  prototype.
- Run 4 (`proto-all4.txt`) is empty: zero findings across the corpus.
- An earlier false positive in `task_runtime_audit` was fixed by clearing
  loans at `core.tasks.join`.

These results come from the prototype, not the Jet code. When this note was
written, the Jet code had not compiled yet: the `SafetyNet-gate` proofq job
(isocheck plus the lint fixture unit) was still in the queue.

### Facts MIR lacks

- **Frozen values (E1113).** Rust lowers `freeze` to a plain copy, and the Jet
  compiler has no `freeze` or E1113 at all. #4619's mutation proof lists E1113,
  but it cannot be covered until a Jet `freeze` port adds a frozen fact
  (`MIRCopyFact.Freeze`).
- **Scalar E0204** (`both(&x, x)` with Int) copies before the call, so it is
  memory-safe in MIR. The fixture uses a non-scalar overlap shape instead.
- **Loans.** MIR has no explicit loan-begin or loan-end operations. The
  proposal's link rules (`link.scoped`, `link.fallback`, `send.link`,
  `noalias.justified`, section 5.8) need them, plus a shape-effect tag on each
  operation.

### Plan for criteria 3 and 4 (`PLAN-3-4.md`)

| Stream | Proof |
|---|---|
| 3a Differential | `run-goldens.mjs --compare-rust`: native stdout and exit must match the Rust path on every golden, with `native-diverges` as its own verdict. The lint corpus scan must report 0 safety findings on the same `.mird` set. |
| 3b Sanitizer | First ASan/UBSan runtime plus Valgrind memcheck over native golden binaries. Later, native codegen emits shadow checks under `JET_NATIVE_SANITIZE=1`. Zero reports over the goldens and the stage-1 self-build. |
| 3c Fuzz | Generated well-typed programs seeded from the memcorpus. Oracles: lint fails after sema accepted means a sema false accept (filed like #4621); native ≠ Rust means a backend bug; a sanitizer report means a runtime bug. A mutation mode deletes one sema check. |
| 4a Runtime unsafe audit | Every raw-memory operation in `Core/**` sits in an audited `#Unsafe` region in the `safety.md` baseline, enforced by extending `check-unsafe-ratchet.mjs` to CoreLib. |
| 4b Bounds and overflow parity | Every unchecked `Index` or `Trap` has a `MIRBoundsFact` proof or an emitted check, and tier-parity goldens give the same panic text and exit code on O0, O1, O2 and web. |

The planned order is 3a with the lint corpus check, 4a in parallel, then 3b,
then 3c (which needs 3a's oracle), and 4b once native trap emission is
complete.

## 5. Runtime I1 defects found the same day (#4638, #4639)

Two memory-safety failures turned up below the checker. Neither the corpus
nor MIR Lint would catch them, which is why criteria 3 and 4 of #4619 exist.

- **#4638.** `JetArena::map_insert*` retained every map value as an exact
  `Int` unless lowering had marked the values as raw words, and only U64 and
  I64 were marked. Positive Floats ≥ 2.0 carry the exact-Int pointer tag bits,
  so `jet_int_retain` dereferenced the mantissa (SIGSEGV). Root cause:
  SegvHunt, `/mnt/jetscratch/scratch/SegvHunt/ROOT-CAUSE.md`. The main routes
  were fixed on `fix/segv-scaling@f520ef1ef`. Three routes remained when the
  card was written: `jet_jit_map_map_values` returning Float, `Map.from_keys`
  with a Float default, and `retain_value_roots` for an embedded map. The
  card's criteria flip the default so that only maps explicitly marked
  exact-Int are retained, and carry the value type in the MIR and host
  signature.
- **#4639.** `Examples/features/memory/pin.jet` faults inside Cranelift JIT
  code (`mov (%r12)` with `r12=0x6a`) on the pre-merge dev binary
  (`dev-05ea86f65`). SegvHunt confirmed it is separate from the map-retain
  bug. The card asks for the miscompiled lowering to be found and fixed, or
  shown to disappear with JIT removal (D-EXEC2), with `pin.jet` passing on
  every remaining tier.

## Limits

- **One binary.** Every verdict is `jet check` or `jet emit --rust` on
  `dev-05ea86f65`. A check pass is a sema verdict, not proof that the program
  runs on every tier.
- **Costs are estimates.** The paired timing job failed on environment setup
  (section 3), so no cost ratio is measured. Site counts are static, and Core
  lowered into a program counts once per program.
- **SafetyNet is unproven in Jet.** The false-positive numbers come from the
  JavaScript prototype over Rust-path MIR dumps. The Jet implementation had
  not compiled when this note was written.
- **Case numbering differs from the proposal.** The proposal cites a 64-case
  corpus, while `RESULTS.md` has 87 programs, and the two classify two cases
  differently. The proposal counts C36 as a safe extra accept; `RESULTS.md`
  and #4621 count 36 as a static false accept. The proposal lists C52 as a
  false reject (a whole-list write window cannot `push`). `RESULTS.md` marks
  52 as agreeing, but its Jet program writes an element through a window
  (`c.out[0] = "x"`) where the Rust original pushes, so the push shape is
  untested there. [INFERENCE: the 64-to-87 difference means the proposal was
  written against an earlier snapshot of the corpus.]
- **Hand classification.** The 920 heap-copy figure extrapolates a 60-site
  sample. The audit tool reports no types.
