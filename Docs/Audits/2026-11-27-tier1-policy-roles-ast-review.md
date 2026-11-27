# Pre-Build Adversarial Audit — #7061 Tier 1 Collapses

**Scope:** #7061 criteria c2–c4 and the c1 audit gate. The card covers four parts:
(1) the tighten-only policy law, (2) the role-trait law, (3) AST follows surface, and
(4) the `spec.md` trim plus a CI guard for fenced `jet` blocks.

**Checkout:** `master` at `56bf8b0811d`. The audit was read-only. No compiler was
built and no test was run. Every count below comes from text searches (grep/awk)
over this checkout.

**Source audit:** `Docs/Audits/2026-11-27-cognitive-budget-orthogonality-audit.md`
(§2.2 D1–D7, §2.3 law 3, O1, O9, O18, O34, O37, O38, T9, T18, and the §5 Tier 1
rows 1–3).

---

## 0. Verdict

**Do not start building yet.** Three of the four parts have a blocking conflict
with Tower state or with code facts that the card text does not account for.

| # | Blocker | Where it bites | Required action |
|---|---|---|---|
| B1 | **The card has no ballot behind it.** The card body says "Owner-ratified Tier 1 collapses". However, `card show 7061` lists `decisions: []`, and the source audit gates Tier 1 row 1 (policy law) as **Ballot** and row 2 as **Ballot for public role-trait names**. `decision list` returns no ratified decision that deletes `#Policy(gc)`, `#Overflow` or `copies:`. | c2, c3 | Post the ballots in §1.6 and §2.5 before building. Under AGENTS precedence, a ratified Tower decision outranks the audit, so the card cannot ratify itself. |
| B2 | **Deleting `#Policy(gc)` reverses ratified decisions.** Several ratified decisions establish scoped `#Policy(gc)`, the package `gc: true` key, and the "opt in the types declared under it" shape. They are visible in `Tools/Tower/.tower/history/decisions-001.json` and `decisions-003.json`. | c2 | Needs a superseding ballot that names the decisions it retires and the replacement for cyclic graphs (`Shared.Weak<T>` or `Pool<T>`/`ID<T>`). |
| B3 | **The role-trait mechanism contradicts a ratified decision.** The ratified decision titled "How a type says it never copies, stays on its thread, or must be used up" chose **option A: header markers** (`#MoveOnly`, type-level `#Local`, `#SingleUse`). It explicitly declined **option B: member-less traits** (`impl MoveOnly`, `impl Local`). Those markers exist in `Compiler/Prelude/Markers.jet:411,420,547`. The card's `impl ThreadConfined` is option B again. | c3 | Use the ratified markers for the thread-confinement and move-only facts. Reserve traits for roles that carry required methods (§2.3). Any other choice needs a re-ballot, and AGENTS forbids re-balloting a settled choice without new evidence. |
| B4 | **Deleting `failures:` collides with #7047 c3** (open, card is `ready`). That criterion keeps "failure-mark enforcement and `jet fmt --explicit-failures`" and only respells `?` as `!`. #7061 c2 deletes the same policy. | c2 | Sequence the two cards or ballot first. One of the two criteria has to change, and only the owner can change it. |
| B5 | **"~150 string matches" is not a checkable scope.** Counting `==`/`!=` against a quoted Core role-type name across `Compiler/JetSema/Source/**/*.jet` gives **293 sites in 61 files** (§2.1). Many of those sites are typing rules for compiler-intrinsic types, not capability grants. | c3 | Replace "~150" with a defined guard (§2.6). Otherwise c3 cannot be met honestly: AGENTS forbids "close enough" evidence and narrowing a criterion. |

Part 3 (internal AST collapse) needs no ballot. It still carries one design trap:
`#loop` formatter fidelity (§3.4). Part 4 is unblocked, with one cutover hazard:
line-number citations of `spec.md` (§4.3).

**Pareto order** (largest share of the gap first, per AGENTS "biggest wins first"):

1. Part 3: about 370 match sites per Item kind across about 130 files.
2. Part 2: 293 name-keyed sites across 61 Sema files.
3. Part 1: about 300 source, test and doc lines, 4 policy keys.
4. Part 4: docs plus 1 guard.

Parts 1 and 2 are ballot-gated. So the order that can actually run is Part 3 now,
Part 4 now, and Parts 1–2 after the ballots.

---

## 1. Tighten-only policy law (c2)

### 1.1 What each switch actually does today (code, not spec)

The policy registry is `Compiler/JetFoundation/Source/Policy/Policy.jet:198-226`
(`POLICY_RULES`). `PolicyKey` is in `Compiler/JetFoundation/Source/AST/Statements.jet:86-113`.

| Switch | Combine rule | Effect in code | Changes meaning? | Law verdict |
|---|---|---|---|---|
| `#Policy(gc)` / `gc: true` (`PolicyKey.ScopedGC`) | **Override** (`Policy.jet:216`), the only non-Tighten package key | Sema only. `FunctionMemory.jet` builds `GCPromotion` facts and emits E2111 (`FunctionMemory.jet:593`). `jet inspect choices` prints "collected under …" (`JetCLI/Source/CLI/InspectChoices.jet:143`). **Nothing lowers it.** `gc_promotion`/`gc_transferred` get **0 hits** in `Compiler/JetIR/Source`, `Compiler/JetBackend`, `Compiler/JetRuntime/Source`, `Compiler/Prelude` and `Core`. The only downstream read is `JetDriver/Source/Driver/Pipeline.jet:4497` (`reach.everything = true`). No collector symbol exists in the runtime. `SharedCycles.jet` (E0221) never consults the policy. | **Diagnostics and reports only. No runtime meaning was found.** | Violates the law in form (Override). Its runtime is already the plain ownership semantics. |
| `Mem.Gc` effect | n/a | Deny-only effect leaf (`Compiler/Prelude/Effects.jet:131-138`). Used only in `Tests/Cases/Effects/EveryEffectLeafChecksThroughEveryPath.fixture/{caps,rows}.jet` and `spec.md:2873,3323`. | No (a denial) | Tighten-only already. It becomes vacuous once the gc policy goes, so delete it with the policy. |
| `#Policy(copies: .Explicit)` | Tighten (`Policy.jet:218`) | Adds refusals only: `Ownership/StringViews.jet:34-72`. `FunctionMemory.jet:645` states it: "`copies: .Explicit` only adds refusals". The E-rows at `DiagnosticRows.jet:2231,14652,14688` describe a refusal. | **No.** The source audit's T18 claim ("or a move") has no support in the code read. | **Already tighten-only.** The law does not require deleting it. Deleting it is a dialect cut (D2, "stops compiling"), not a semantics fix. |
| `#Overflow(.Wrapping/.Saturating/.Stop)` (`PolicyKey.Arithmetic`) | **Override** (`Policy.jet:221`, innermost wins) | Routes fixed-width `+ - * << pow` and narrowing conversions (`SyntaxDictionaryData.jet:276`). Read at `FunctionHeaders.jet:369-386` and `Comptime/Reflection.jet:2325-2344`. | **Yes**: `a + b` traps or wraps depending on the enclosing scope. | **Violates the law.** It must go, or become an explicit op or type. |
| `failures: .Explicit` (`PolicyKey.Failures`) | Tighten (`Policy.jet:223`) | Adds E0434 (`FailureMarks.jet:133,159`). **It also removes an error:** a bare `?` is refused *outside* the policy (`DiagnosticRows.jet:6155-6157`, `Prelude/Diagnostics.jet:7036-7047`) and accepted inside it. | No for code that compiles both ways (`?` is a no-op mark). | Passes the meaning test. **Fails the literal c2 wording** "may only add errors", because it lifts the bare-`?` refusal. |
| `sentries: .Off` (**not in the card**) | Tighten (`Policy.jet:219`) | Switches off the runtime witness in sentry builds (`FunctionHeaders.jet:414-421`, `FunctionMemory.jet:649-652`). | No for correct programs. It removes run-time traps. | **Loosens checking.** Under the c2 wording it is in scope, but the card does not list it. Owner question (§1.6 Q5). |

Also outside the card but named in Tier 1 row 1 of the source audit: the derive
opt-out (D4) and `#NoPrelude` (D5). The card body lists neither, so leave them out
unless a ballot adds them.

### 1.2 Census of switch uses (exact hits / files)

Counted with awk over `*.jet *.md *.case *.stderr *.out *.txt *.header`:

| Pattern | Core | JetRuntime | Prelude | Examples | Tests | JetSema | JetParser | JetFoundation | JetCLI | JetDriver | Jetpack | Docs/Spec |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| `#Overflow(` | **86 / 13** | 1 / 1 | 1 / 1 | 4 / 3 | 45 / 26 | 11 / 6 | 5 / 4 | 5 / 4 | 1 / 1 | 2 / 2 | 0 | 0 |
| `#Policy(gc)` or `gc: true` | 0 | 0 | 3 / 1 | 1 / 1 | 12 / 11 | 1 / 1 | 0 | 4 / 1 | 2 / 1 | 0 | 1 / 1 | 2 / 2 |
| `copies: .Explicit` | 0 | 0 | 6 / 1 | 2 / 1 | 79 / 55 | 21 / 7 | 4 / 2 | 9 / 2 | 3 / 3 | 0 | 2 / 2 | 3 / 1 |
| `failures: .Explicit` | 0 | 0 | 5 / 1 | 0 | 18 / 14 | 15 / 8 | 3 / 2 | 5 / 2 | 3 / 3 | 0 | 0 | 2 / 1 |
| `sentries: .Off` | 1 / 1 | 0 | 1 / 1 | 2 / 2 | 21 / 17 | 0 | 2 / 1 | 2 / 1 | 0 | 0 | 1 / 1 | 0 |
| `Mem.Gc` | 0 | 0 | 1 / 1 | 0 | 5 / 2 | 0 | 0 | 0 | 0 | 0 | 0 | 2 / 1 |

The 13 Core files with `#Overflow` are all under `Core/Crypto/` plus
`Core/Net/SSL/TLS.jet` and the doc text in `Core/Compiler/Lang.jet:129`. The Crypto
files are Aead, Bignum, Cipher, ConstTime, Crypto, ECP, Expert, ExpertCt, MD, PEM and
RSA. The one runtime use is `Compiler/JetRuntime/Source/SHA512.jet:122`.

### 1.3 Zero safety/power-loss argument, per switch (attacked)

- **`#Policy(gc)` and `Mem.Gc`.**
  - **Runtime:** deletion is meaning-neutral on this checkout, because no lowering
    consumer exists (§1.1).
  - **Power:** the cycle story is unchanged. E0221 fires with or without the policy,
    since `SharedCycles.jet` has no policy read. The escape hatches are
    `Shared.Weak<T>` (`SharedOwner.jet:515-517`) and `Pool<T>`/`ID<T>`.
  - **Attack:** this deletes a *ratified, partially built feature*, not a dialect
    with live semantics. The honest framing is "retire an unbuilt opt-in", and that
    needs the superseding ballot (B2).
  - **Losses:** E2111, the `jet gc report` command (`JetCLI/Source/CLI/GCReport.jet`),
    the `gc_trace`/`JET_GC_TRACE` control (`JetFoundation/Source/Registry/CLIControls.jet:44`),
    and the "Gc Commands" help sections (`Tests/CLI/help_*.txt`). Removing a command
    is an owner gate under AGENTS.
  - **Proof required:** `Examples/Memory/Allocation/GCCyclic.jet` must print the
    same bytes with and without line 3 on O0, O1 and O2. This proves runtime
    neutrality directly. Do not claim it without that run.
- **`#Overflow`.**
  - **Power:** the per-expression gates already exist. `wrapping(…)`,
    `saturating(…)` and `checked(…)` (`MathLayout.jet:799-801`) are exercised in
    `Examples/LowLevel/FixedOverflowModes.jet:30-35`.
  - **Attacks**, each to become a criterion:
    1. **Coverage.** The switch also governs **compound assignment** (`x += y`,
       `JetIR/Source/BuildTypes.jet:2598`), `.pow`, shifts and **narrowing
       conversions**. A golden must show that `wrapping(…)` covers each of those
       forms, or that a mechanical rewrite exists (`x = wrapping(x + y)`).
    2. **Constant-time risk.** The 85 Crypto function sites run in constant-time
       code. If a rewrite misses one operator, that operator turns from a silent
       wrap into a *checked trap*: a data-dependent branch on secret data and a
       remote DoS vector. The migration must be a `jet fix` rewrite that covers
       whole function bodies. It must be followed by the Crypto known-answer suites
       and the constant-time IR validation on O0, O1 and O2.
    3. **Noise.** The ChaCha, BLAKE and SHA round functions become
       `wrapping(...)`-dense. The alternative that keeps one mark per region without
       a dialect is a **wrapping type** (`Wrapping<U32>` or `W32`). That is new
       public API, so it needs a ballot, plus the paired two-program perf cell AGENTS
       requires for perf-motivated surface.
- **`copies: .Explicit`.** Deleting an error-only switch cannot remove power.
  - **Attack:** it removes an *audit tool*, namely `jet fmt --explicit-copies`
    (`JetCLI/Source/CLI/Front.jet:2968`, `FormatMarks.jet:253`, `Host.jet:4211-4278`)
    and the refusal side of `jet audit copies` (`AuditCopies.jet`). Under I8 the
    better collapse is to **re-home it as a deny-able lint**. That is tighten-only by
    construction and needs no policy key.
  - Deleting it outright removes a CLI flag, which is an owner gate.
- **`failures: .Explicit`.** This switch is blocked by B4. If the policy survives
  (#7047 c3), the c2 law can still hold by **making a bare `?`/`!` legal everywhere
  as a no-op mark**. That deletes the bare-`?` refusal row instead of the policy.
  That choice is the owner's.

### 1.4 Binding cutover list — Part 1 (conditional on the §1.6 ballots)

**Delete `ScopedGC` (gc):**

- **Policy and parser:**
  - `JetFoundation/Source/Policy/Policy.jet`: lines 25, 55, 67, 216, and 314.
  - `PolicyKey.ScopedGC` in `AST/Statements.jet:105`.
  - The `gc` arm of `JetParser/Source/Parser/Declarations/Policies.jet` and its
    E0355 fix text (`:205`).
- **Sema:**
  - Every `ScopedGC` branch in `JetSema/Source/Sema/FunctionMemory.jet`; the file's
    purpose is scoped GC.
  - `Finalize.jet:298,305`.
- **AST and checked-tree facts:**
  - `GCPromotion` and `GCPromotionEdge` (`AST/Statements.jet:219-232`).
  - `Binding.gc_promotion` and `gc_transferred`.
  - `TGCPromotion` and `TGCPromotionEdge` (`Facts/Checked.jet:316-325`).
  - The `StmtKind.Let` fields (`Facts/BodyStmt.jet:11`).
- **All builders of those fields:**
  - `Calls/{Build,Checking,Direct,HTTPMount,Methods,Values}.jet`.
  - `BodyFlatten.jet:579,1166`.
  - `JetIR/Tests/{CloseShield,PowerAssert}.jet`.
  - `JetParser/Tests/ASTCodec.jet:177-179`.
  - `JetDriver/Source/Driver/Pipeline.jet:4492-4497`.
- **Regenerate:** `Facts/SyntaxCodec.jet`, `Facts/BodyCodec.jet` and
  `Sema/BodyPacketCodec.jet` via `Compiler/Toolchain/BodyCodec/Main.jet --write`, then
  `--check`.
- **Diagnostics:**
  - E2111: registry row `DiagnosticRows.jet:16606-16620`,
    `Prelude/Diagnostics.jet:20162-20176`, and the UI pair
    `Tests/UI/gc_escape_ownership.{jet,stderr}`.
  - `Tests/Fuzz/Sema/Invalid/ui_gc_escape_ownership.E2111.jet`.
  - Retire the code; never reuse it (I4).
  - E0355 fix text (`DiagnosticRows.jet:5150-5154`, `Prelude/Diagnostics.jet:5816`),
    plus the snapshots `Tests/CLI/inspect_guarantees_source_policy.txt` and
    `Tests/Cases/CLIPartsInspect/…`.
- **CLI (owner gate):**
  - `GCReport.jet` with its test.
  - The `gc_trace` control.
  - The collection arm of `InspectChoices.jet`.
  - Help and completion snapshots (`Tests/CLI/{help_az,help_frequency,completions_zsh}.txt` and the `Tests/Cases/CLIParts*` copies).
  - `Tests/CLI/memory_fix_ambiguous.txt` and
    `Tests/Cases/MemorySemantics/{inspect_choices_shows_collection,memory_fix_applies_one_exact_repair…}.case` with fixtures.
- **Corpus:**
  - `Examples/Memory/Allocation/GCCyclic.jet` and its golden, rewritten to `Shared.Weak` or `Pool`/`ID`.
  - `Tests/Fuzz/Sema/Valid/ex_memory_gc_cyclic.jet`.
  - `Tests/Cases/PolicyScope/PackageGCPolicyAppliesOnceAcrossImportedModules.fixture`.
  - The package `gc` key in Jetpack (1 file).
- **`Mem.Gc`:**
  - `Prelude/Effects.jet:131-138`.
  - The two Effects fixtures.
  - `spec.md:2873,3323`.
- **Docs:**
  - `spec.md:4145-4148`.
  - `Docs/Spec/Reference/core-library.md:378`. That line also carries ratification
    IDs; drop them in the same edit.

**Delete `Arithmetic` (`#Overflow`):**

- **Registry and parser:**
  - `MARKER_OVERFLOW` (`Data/Syntax/CoreSurface.jet:532-538`, `Policy/MarkerNames.jet:70`).
  - The Prelude marker struct (`Prelude/Markers.jet:41`).
  - `PreludeMarkers.jet:47`.
  - `overflow_*` in `Policy.jet:94-128`.
  - The `Arithmetic` key and value.
  - Parser `Statements.jet:2553,2592` and `Declarations/Policies.jet:77-87`.
  - Formatter `Statements.jet:263`.
- **Sema and indexing:**
  - `FunctionHeaders.jet:369-386`.
  - `Comptime/Reflection.jet:2325-2344`.
  - `JetSemIndex/Source/SemIndex/Build.jet:1529`.
  - `JetDriver/Source/Driver/CoreReach.jet:344` and its test at `:709`.
- **CLI:** the syntax-dictionary row (`SyntaxDictionaryData.jet:276`; it is
  generated, so regenerate it).
- **Migration:**
  - The 86 Core and 1 runtime sites through one `jet fix` rule.
  - `Examples/Interop/Murmur3/{run.jet,Provenance.md}`.
  - `Examples/LowLevel/FixedOverflowModes.jet` lines 34-35 and its golden.
  - 45 test lines in 26 files, including the CompilerAPI fixture and the Fmt fixtures.
- **Keep:** the `Overflow` *event type* (`MathLayout.jet:179`, `RuntimeHandles.jet:311`,
  `CoreRecordRows.jet:1099,1172`). Only the name collides; it is not the policy.

**`copies` and `failures` (outcome depends on the ballot):** if deleted, the keys,
the `StringViews.jet:34-72` copies gate, the `FailureMarks.jet` pass, the
`--explicit-copies`/`--explicit-failures` fmt modes, and 79+18 test lines go. If they
are re-homed as lints, the keys go and the checks become lint rows with UI snapshots.

### 1.5 Proof plan for c2

1. A golden or UI pair per deleted switch on O0, O1 and O2 (I5, I9). It shows the
   migrated program's output, plus the parser error for the removed spelling with a
   `jet fix` edit.
2. A structural guard: every `POLICY_RULES` row is `combine: Tighten`. Assert this in
   a `Policy.jet` `#Test`, so Override cannot return.
3. The Crypto known-answer suites and the constant-time validation on O0, O1 and O2
   after the `#Overflow` rewrite.
4. A full suite, because this is a cross-cutting sema and Core change.

### 1.6 Ballots to post (owner gates)

- **Q1:** retire `#Policy(gc)`, the `gc` package key, `Mem.Gc`, E2111,
  `jet gc report` and `JET_GC_TRACE`. This supersedes the ratified scoped-GC opt-in
  decisions.
- **Q2:** replace `#Overflow` with (a) `wrapping`/`saturating`/`checked` only, or
  (b) a wrapping integer type. Option (b) needs the paired perf cell.
- **Q3:** for `copies: .Explicit`, choose delete, a deny-able lint, or keep (it is
  already tighten-only).
- **Q4:** for `failures: .Explicit`, choose between deleting it (and amending #7047
  c3) and keeping it with bare `?`/`!` legal everywhere.
- **Q5:** decide whether `sentries: .Off` falls under the law.

---

## 2. Role-trait law (c3)

### 2.1 Exact inventory

The search was `==` or `!=` against a quoted name from {Cell, CellReadGuard,
CellEditGuard, Atom, Atomic, Mutex, SharedGuard, Task, Sender, Receiver,
ProcessChild, TempDir, TempFile, MappedFile, FileScope, FileReader, FileWriter,
FileLock, ID, Pool, Arena, Bump, Fixed, View, ViewMut, ViewIter, Iter, Stream, Dyn,
JSON, TOML, YAML, CSV, DateTime, ZonedDateTime, Date, LocalDate, LocalTime,
Duration, Instant, Weak, Shared, Set, Secret, Authority}, over
`Compiler/JetSema/Source/**/*.jet` (236 files). It found **293 sites in 61 files**.

Counts by name: ViewMut 29, View 28, Iter 15, Dyn 15, Stream 12, ViewIter 11,
Duration 11, Cell 10, SharedGuard 9, Secret 9, Pool 9, DateTime 9, CellEditGuard 9,
Receiver 8, Set 7, CellReadGuard 7, ZonedDateTime 6, LocalTime 6, LocalDate 6, JSON 6,
Instant 6, Task 5, Sender 5, ID 5, FileReader 5, Date 5, YAML 4, TOML 4, Shared 4,
CSV 4, TempDir 3, Fixed 3, TempFile 2, Bump 2, Authority 2, Atomic 2, Arena 2, and
1 each for Weak, ProcessChild, Mutex, MappedFile, FileWriter, FileScope, FileLock
and Atom.

The search **misses** suffix and list forms. Examples: `name.ends_with(".TempDir")`
(`Ownership/Copy.jet:1396`), `name.ends_with(".Atomic")` (`CoreUsage.jet:736`), and
the `<corelib>/core.mem.Arena` identity constant (`Calls/Memory.jet:85`). The guard
in §2.6 must cover those too.

Grouped by the role each match encodes (file:line on this checkout):

| Proposed role | What Sema grants or denies by name | Sites |
|---|---|---|
| **Thread-confined** (Cell family) | Cannot cross threads, cannot enter `Shared` | `Ownership/Crossings.jet:353,522`; `Calls/Static.jet:907-911` (E1102); `SharedAccess.jet:55,68,94`; `Ownership/CellGuards.jet:24`; `Calls/SharedOwner.jet:411-449` (Cell method typing); `Scopes.jet:3491,3518`; `CoreUsage.jet:218,229` |
| **Guard / acts-when-dropped** | Not copyable, has a drop duty, not comparable | `Ownership/Copy.jet:1396,1485`; `Comparability.jet:1023`; `Calls/Memory.jet:434`; `Expressions/Places.jet:358`; `Statements/Bindings.jet:169`; `Ownership/Checks.jet:822,876`; `Registration/TypeMetadata.jet:43`; `Calls/Checking.jet:1883` |
| **Handle** (resources, channel ends, tasks) | Not comparable; special crossing; `#SingleUse`-like | `Calls/Core.jet:1494,1513,1529`; `Calls/Concurrency.jet:526,553`; `Registration/BuiltinOwners.jet:54`; `Registration/Checked.jet:1325`; `Ownership/TaskGroups.jet:466,1257,1386`; `Statements/ForIn.jet:368,1332,1349`; `TypeRelations.jet:2785`; `Calls/Methods.jet:2723-2769` |
| **Allocator** | Allocator family typing and effects | `Calls/Memory.jet:96,358`; `Effects/Checks.jet:5218`; `Expressions.jet:5952` |
| **Pool/ID indexing** | `pool[id]` typing; `add`/`remove`/`ids` | `Calls/SharedOwner.jet:524`; `Expressions/Literals.jet:1075,1269,1330`; `SharedAccess.jet:360`; `Statements/ForIn.jet:1016`; `Comparability.jet:630,1022`; `Calls/Reflection.jet:443`; `TypeRelations.jet:2983` |
| **Borrowed** (View family) | Second-class: no store, no return, no crossing | 72 View/ViewMut/ViewIter sites across `Ownership/{Checks,Crossings,CopySites,SingleUse}.jet`, `Statements/{Views,Returns,Bindings,Assignments,ForIn}.jet`, `Expressions/{Constructors,Lambdas,Literals,Places,Control}.jet`, `ReturnViews.jet`, `Guest.jet` and `Expressions.jet` |
| **Iteration protocol** | Element type of `Iter`/`ViewIter`/`Stream` | `Calls/{Collections,Direct,Methods,Reflection}.jet`; `Expressions/{Zip,Literals}.jet`; `Statements/{ForIn,Loops}.jet`; `Ownership/MustUse.jet:18`; `FunctionCheck.jet:163`; `FailureSolve.jet:37`; `WebPartition.jet:598,620` |
| **DynamicData** (formats and dates) | Dyn literal coercion, derive and serde shortcuts | `Expressions/Dyn.jet:45,141,254,358,461`; `Expressions/Literals.jet:585,713`; `Registration/Derives.jet:2669-2678`; `Registration/OwnedTraitContracts.jet:294`; `Registration/SerdeItems.jet:217`; `Serde.jet:279-353`; `Calls/Plugin.jet:97`; `Calls/Reflection.jet:382`; `Calls/HTTPMount.jet:1972`; `TypeRelations.jet:1123,3056`; `Taint.jet:487`; `Calls/Data.jet:351` |
| **Secret / Authority** | Taint and authority handling | `Effects/Checks.jet:6044,7650`; `Serde.jet:301,341`; `Registration/SerdeItems.jet:348,360`; `Diagnostics.jet:926`; `Calls/Methods.jet:242` |

### 2.2 Attacks on the "~150 sites → ~0" claim

1. **Most sites belong to compiler-intrinsic types, not Core library types.**
   - `TYPE_NAMES` (`JetFoundation/Source/Syntax/Spellings.jet:107-155`) registers
     `Shared`, `SharedGuard`, `Task`, `Receiver`, `Sender` and more as **built-in
     types**.
   - `Crossings.jet:356-360` says that "a carrier the compiler types itself
     (`Atomic<Int>`, `SharedGuard<T>`, `Task<T>`) has no nominal row".
   - Cell methods are typed by Sema (`SharedOwner.jet:411-449`), not by a Core
     declaration.

   A role trait cannot remove those matches until each type is an **ordinary Core
   declaration** with real method signatures. That work sits in #7059
   (Shared/Weak/Pool/ID) and #7056 (one Core SSoT). **c3 depends on both.** List
   them as `blockedBy`, or move those types out of c3's scope by ballot.
2. **The View family is language, not library.** View, ViewMut, ViewIter and Pin
   are second-class borrows whose soundness the borrow checker proves.
   - A user-declarable `Borrowed` role that *grants* view powers (lending, or
     `View<str>` materialization) would be **unsound under I1**: Sema cannot prove a
     user type's internal lifetime discipline.
   - A role that only *restricts* (cannot store, return or cross) is sound, but adds
     nothing over `#Local` plus `#MoveOnly`.
   - **Recommendation:** register View, ViewMut and ViewIter as built-in types (a
     `Type` kind, or one `type_is_view` predicate over a registry constant). Then the
     72 sites query one fact. This is the Pillar 5 boundary: language types may be
     known to the compiler, and library types may not. No `Borrowed` trait.
3. **Grant roles and restrict roles differ in safety.**
   - Restricting roles (thread-confined, move-only, single-use, acts-when-dropped)
     are safe to let any package declare.
   - Granting roles are a soundness hole unless they are traits whose required
     methods Sema checks: allocator family typing, `pool[id]` index typing, and Dyn
     literal coercion.
   - So the minimal set is **not** six symmetric traits.
4. **The ratified marker decision already covers three of the six facts** (B3).
   #6852 c1 and c2 are `met` for Copy, Crossings and Typestate. Yet `Copy.jet:1396`
   and `Crossings.jet:353,522` still name Cell, SharedGuard, ProcessChild and
   TempDir.
   - Either this is residue the `met` evidence missed, or it is the intrinsic-type
     carve-out above.
   - Treat it as a defect on #6852 or #7061 with exact lines. Do not open a
     duplicate card.

### 2.3 Concrete minimal role set (proposal for the ballot)

| Fact | Mechanism | Public surface | Replaces |
|---|---|---|---|
| Stays on its thread | **`#Local` on the type** (ratified) | none new | Cell family names in Crossings, SharedAccess and Static E1102; `ThreadConfined(name)` becomes `DeclaredLocal` |
| Never copies | **`#MoveOnly`** (ratified) | none new | Copy.jet guard names; Atomic at `Copy.jet:375` |
| Must be consumed | **`#SingleUse`** (ratified) | none new | Task join/detach, already done per #6852 |
| Acts when dropped (L0101 exemption) | Derived from `impl Close` or a drop duty at any field depth. `sema_copy_carrier_declares` already exists (`Copy.jet:1392`). | none new | `Copy.jet:1396` list |
| Not comparable | Absence of `Equatable`/`Comparable`. Intrinsic types simply do not implement them. | none new | `Comparability.jet:1022-1023` |
| Allocator family | **`trait Allocator`** with required `alloc`/`reset` signatures; the family is read from the impl | **new trait name: ballot** | `Calls/Memory.jet:96,358`, `Effects/Checks.jet:5218` (and #6852 c16's deleted static) |
| Keyed store `S[K] -> V` | An **`Index`-style trait** (`impl Index<ID<T>>`), to be checked against Prelude `Traits.jet` before adding | ballot only if no existing trait fits | Pool sites in Literals, SharedOwner, ForIn and SharedAccess |
| Iteration element | The existing iteration traits (Prelude `Traits.jet`); #6852 c8 moved line loops to public traits | none new | Iter/ViewIter/Stream element-type sites |
| Dynamic data | **`trait DynamicData`**, or reuse `Encode`/`Decode` plus one `From<Dyn>` impl | **ballot** | Dyn.jet, Derives, OwnedTraitContracts, Serde, Plugin and HTTPMount lists |
| View family | **Built-in type fact, not a trait** (§2.2 attack 2) | none new | 72 View sites |

That gives three new trait names at most (Allocator, possibly Index, DynamicData),
against the card's six. `Guard`, `Handle` and `Borrowed` collapse into existing
markers, traits or built-in facts.

**Prior art to generalize:** `Sema/CoreRoles.jet` (`#Builtin.Role(.X)`) already finds
Core *functions* by declared role instead of by name. However, it is gated by
`module_is_core` (`CoreRoles.jet:14`). That is still a Core-only power under
Pillar 5. The role law should not copy that gate.

### 2.4 `.new` has hidden compiler meaning today

The criterion asks to "verify `.new` carries zero compiler magic". It does not hold
on this checkout. Each item below must be deleted or made general:

1. **E0181** (`Registration/Checked.jet:2533-2546`): a static method *named* `new`
   must return its owner type. This is a name-keyed rule. Delete it or extend it to
   every static method by a declared marker; the second option is a ballot.
2. **Inferred-receiver `.new(…)`** (`Expressions.jet:8102-8106`): only the method
   name `new` gets expected-type receiver inference. Make it uniform for any static
   method with the expected type (a ballot, because the user surface grows), or
   delete the shorthand (also a ballot).
3. **Call-as-constructor rewrite** (`Calls/Direct.jet:426-430`): `Decimal(…)` and
   math types are rewritten into a synthesized `Name.new(…)`. A hidden rewrite keyed
   on the method name.
4. **Name-keyed `new` special checks:**
   - `Calls/Static.jet:907` (`Shared.new`, E1102), `:1471` (`ExpiringSecret.new`),
     `:2008`.
   - `Scopes.jet:3518` (`Cell.new`).
   - `SharedCycles.jet:214,458`.
   - `Comptime/Fold.jet:85` (`JetShared.new`) and `:133` (`core.event.new`).
   - `Effects/Checks.jet:5208`.
   - `Calls/Core.jet:530` (`core.time.expiring.new`).
   - `Calls/Game.jet:92` and `BudgetSpecs.jet:298` (`Scene.new`).
   - `CoreUsage.jet:1201`.
   - `Expressions/Constructors.jet:386` (`Atomic.new` builtin row).
5. **Synthesized calls:** `Expressions/LoopValues.jet:368` builds `Set.new()`.

Proof for "zero magic": a guard finding no `"new"` string literal in
`Compiler/JetSema/Source`, plus a user type with a `new` that returns a non-owner
type and compiles like any static method.

### 2.5 Binding cutover list — Part 2

- **Before c3 can close:** #7059 c3 and c4 (Shared/Weak/Pool/ID as ordinary Core),
  and #7056's Core SSoT for Cell, Atomic, SharedGuard, Task, Sender and Receiver
  declarations.
- **Per role row in §2.3:** delete every listed site and route it through
  `sema_copy_carrier_declares` (marker facts) or a trait-impl query. Add a
  **user-type twin** golden per role: a package type declared the same way gets the
  same diagnostic, using the E1102 and E0221 UI snapshots.
- **View family:** introduce one built-in predicate and convert the 72 sites.
- **`.new`:** handle the §2.4 items 1–5. Items 1 and 2 are ballot-gated.
- **Proof:** O0, O1 and O2 goldens per role, the user-twin UI snapshots, and the
  full suite (as c3 requires).

### 2.6 Guard that makes c3 checkable

Add a `Tools/Agent/` Jet ratchet modelled on the existing `UnsafeRatchet.jet` and
`PanicRatchet.jet`:

- It scans `Compiler/JetSema/Source/**/*.jet`.
- It fails on `==`, `!=`, `ends_with(".X")`, `[String]{"X"…}`/`contains("X")` and
  `<corelib>/…X` forms for any name in a registry list. The list is generated from
  Core's exported type names, not hand-kept.
- It starts with a committed baseline count and allows decreases only.

c3 closes when the baseline is **0 outside an explicit, reviewed allow-list of
language built-ins** (the `TYPE_NAMES` rows). This replaces the "~150" text and
needs a criterion edit by the owner (B5).

---

## 3. AST follows surface (c4, internal)

### 3.1 Variants to delete and their blast radius

Counts are `.<Variant>(` hits in `Compiler/**`, `Jetpack/**` and `Tools/**` `*.jet`:

| Variant | Hits / files | Target |
|---|---|---|
| `Item.MarkerDecl` (`AST/Items.jet:26`) | 370 / 132 | `Item.Struct` + `impl Marker{…}`. Role data comes from the impl's typed constants; `Prelude/Markers.jet` already declares them this way. |
| `Item.EffectDecl` (`:12`) | 359 / 130 | `struct` + `impl Effect` |
| `Item.Tag` (`:11`) | 377 / 141 | `struct` + tag role impl |
| `Item.FactDecl` (`:27`) | 354 / 130 | `struct` + fact role impl (`value.fact` source) |
| `Item.UnitFamily` (`:9`) | 388 / 139 | Named in source-audit Tier 1 row 3 but **not in c4's text**. Its surface move (`distinct` + `Unit`) is Tier 3 row 21, which is ballot-gated. Keep it out of #7061. |
| `Item.TemplateLoop` (`:23`) | 356 / 129 | Desugar to `#run` (§3.4) |
| Jumps: `Break`, `BreakValue`, `Continue`, `BreakLabel`, `BreakLabelValue`, `ContinueLabel` (`Statements.jet:11-16`) | e.g. `.ContinueLabel(` 223 / 81, `.BreakLabelValue(` 167 / 69 | `Stmt.Jump(kind: Break\|Continue, label: NamedSpan?, value: Expr?, span)` |
| Loops: `While`, `For`, `Loop`, `CountedLoop` (`:8,9,17,18`) | `.CountedLoop(` 221 / 89 | `Stmt.Loop(head: LoopHead, body, span, arrow_body, label)` with `LoopHead = Forever \| While(cond) \| For(...) \| Counted(...)` |
| Marked blocks: `Unsafe`, `Impure`, `Reactive`, `Shield`, `Region`, `Policy`, `Layout`, `ContextBlock`, `Live`, `AssumeDet`, `Transact` (11) + generic `Switched` (`:19-35`) | e.g. `.AssumeDet(` 164 / 69, `.Transact(` 216 / 87, `.Switched(` 165 / 69 | `Stmt.Marked(marker: Marker, kind: MarkedKind, body, span)` |

Net effect: `Stmt` goes from 36 kinds to 36 − 6 − 4 − 12 + 3 = **17**, and `Item`
from 24 kinds to **19** (five removed: MarkerDecl, EffectDecl, Tag, FactDecl,
TemplateLoop).

### 3.2 Attacks

1. **The `Stmt.Marked` payload problem.** The 11 bespoke kinds carry typed payloads:
   `Unsafe.audit` (I1-bearing), `Policy.declarations`, `Region.name`,
   `ContextBlock.fields`, `Transact.implicit`, `AssumeDet.reason`.
   - If `Stmt.Marked` holds only the raw `Marker`, every pass re-parses the marker
     arguments. That duplicates diagnostics, breaks I3 (sema-owned parsing in one
     place), and costs compile speed.
   - **Requirement:** `MarkedKind` is a typed enum resolved once at parse or
     registration time, and passes that don't care match one arm.
   - The honest gain is "one arm for passes that ignore the payload", not "11
     variants deleted". The cost moves into `MarkedKind`. State this in the
     closeout.
2. **`#Unsafe` must stay exact.** `UnsafeObligations.jet`, `GateLedger.jet` and
   `Tools/Agent/UnsafeRatchet.jet` key on the Unsafe statement. The collapse must
   keep a direct `MarkedKind.Unsafe(audit)` arm. It must also keep the
   audited-region spans the ratchet counts, and run the ratchet before and after
   with identical counts.
3. **Role-Item collapse needs ordering.** Marker and effect facts are needed before
   the passes that apply them: `MarkerModules.jet`, `MarkerRules.jet` and
   registration. The collapse needs **one early role-registration pass** that
   derives "is a marker, effect, tag or fact" from `impl Role` facts. Without it,
   each pass re-scans impls. The source audit notes this trade-off (O1); make it an
   explicit criterion.
4. **Parser-synthesized impls.** `JetParser/Source/Parser/Declarations/Markers.jet:151`
   synthesizes `TraitImplBlock{compiler_generated: true}` for markers. After the
   collapse that synthesized impl *is* the declaration, so `compiler_generated`
   semantics must not hide it from `jet inspect` or doc generation.

### 3.3 Codec impact

- `Facts/SyntaxCodec.jet` (7945 lines) is **generated** by
  `Compiler/Toolchain/BodyCodec/Main.jet` ("do not edit"; `--write` and `--check`).
  Every variant deletion or reorder changes the variant positions it writes.
  - Regenerate it in the same commit.
  - Run `--check` byte-for-byte.
  - Bump the packet or cache format version so a stale cached `Program` cannot
    decode into the wrong variant. Check where the codec version is stamped before
    building.
- `Sema/BodyPacketCodec.jet` and `Facts/BodyCodec.jet`: these are only affected if
  `TStmt` or `StmtKind` change. The Part 3 collapse is AST-level, so `BodyCodec`
  changes only through the gc fields in Part 1.
- `Sema/PreludeRows.jet` is generated and embeds **AST literals**, for example
  `TraitImplBlock{… compiler_generated: true …}` at `:338,342`. Regenerate it after
  the AST change.
- `JetParser/Tests/ASTCodec.jet` enumerates AST types (`:177-179`); update it.

### 3.4 `#loop` → `#run` parser desugar while keeping `#loop` on the frontend

**The trap.** The formatter reads the same parser AST:

- `JetParser/Source/Formatter/Items.jet:785,887` (`jfmt_fmt_item_template_loop`).
- `Formatter.jet:624,677`.

If the parser lowers `#loop` to `Stmt.ComptimeBlock`, `jet fmt` rewrites user
`#loop` as `#run`. That breaks "keeping `#loop` on the frontend" and fmt
idempotence.

**Options:**

- (a) Keep the origin: `ComptimeBlock{…, origin: SurfaceOrigin.TemplateLoop(var, var_span, source)}`.
  The existing `is_template_loop: Bool` (`AST/Statements.jet:31`, 22 hits / 10
  files) already half-does this. Widen it into the origin record, and the formatter
  prints from the origin.
- (b) Desugar in a post-parse lowering step that the formatter path does not run.

**Recommendation: (a).** It is one node, the formatter stays a pure printer, and
SemIndex/LSP spans and `jet inspect expand` can show both forms.

Out of scope (Tier 3 row 19, ballot): `#build`/`#here` (fact roots, `ComptimeName`
at `AST/Expressions.jet:59`), `#run fn` (`Func.is_comptime`, `AST/Items.jet:964`)
and `#run use` (`ImportDecl.is_comptime`, `:1422`). These are already **flags on
existing nodes, not separate variants**, so c4 has nothing to collapse there. Leave
them alone and note that in the closeout.

### 3.5 Binding cutover list — Part 3

1. **AST:** `JetFoundation/Source/AST/{Items,Statements,Walk,Nodes,Comptime}.jet`.
2. **Generated:** `Facts/SyntaxCodec.jet`, `Sema/PreludeRows.jet` and the
   syntax-dictionary data; regenerate and `--check`.
3. **Parser:**
   - `Parser/{Items,Statements,ConfigProgram,ImplPlacement}.jet`.
   - `Declarations/{Markers,Effects,Policies,InBodyMarkers,StateMarkers,Modules}.jet`.
   - `ExpressionForms/Loops.jet`.
4. **Formatter:** `Formatter/{Formatter,Items,Statements}.jet`, with fmt idempotence
   cases for every marked block and for `#loop`.
5. **Sema:** every file in the 130–140 file sets above. The largest are
   `MarkerModules`, `MarkerRules`, `Registration/*`, `FunctionMemory`, `BodyPasses`,
   `Statements/*`, `Flow`, `Liveness`, `Divergence` and `LoopFacts`.
6. **Downstream:** `JetSemIndex`, `JetDriver` (`CoreInterface`, `Pipeline`),
   `JetCLI` (`inspect expand`, `LSP`, `FormatMarks`), and Jetpack config parsing.
7. **Guard:** a `#Test` in `AST/Statements.jet` that asserts the `Stmt` and `Item`
   case counts (17 and 19), so variants cannot creep back.
8. **Proof:** the full suite, plus `UnsafeRatchet` with unchanged counts and the
   fmt corpus (`Tools/Agent/FmtCorpus.jet`) byte-identical.

---

## 4. `spec.md` trim and fenced-block guard (c4)

### 4.1 Facts

- `Docs/Spec/spec.md` has 8550 lines and 40 `##` sections.
- Fences by info string: `jet` 67, `jet ignore` 2, `text` 14, `json` 2, `ebnf` 2,
  `toml` 1.
- 592 lines mention E-codes or counted inventories.

### 4.2 Trim plan: keep durable contracts, point to the live single source of truth

| Section (start line) | Action | Live SSoT to point at |
|---|---|---|
| Lexical structure (43) | Keep the rules and tokenization law; **cut keyword, sigil and type tables** | `JetFoundation/Data/Syntax/Spellings.jet` (`LEXICAL_LEDGER`, `LANGUAGE_REWRITES`), `Source/Syntax/Spellings.jet` (`KEYWORD_NAMES`, `TYPE_NAMES`) |
| Core grammar (403) | Keep; the `ebnf` blocks are the contract | — |
| Values … Errors as values (488–2295) | Keep the semantics. **Fix the stale forms** (O38): the old lambda shape, comptime `@`, `?` propagation, and `#Policy(failures:)` at 2148, depending on Q4 | — |
| Ownership, Access sigils, Boundary crossings (2321–3044) | Keep the law; drop `#Policy(copies:)` prose (2339, 2475, 2538) per Q3 | — |
| Effect system (3044) | Keep the lattice law; **cut the effect-leaf tables** (3323) | `Compiler/Prelude/Effects.jet` |
| Expert memory tier (3841) | Cut the policy-key table (3897-3926) and `#Policy(gc)` (4145) | `Policy.jet` `POLICY_RULES` |
| Imports … Concurrency (4150–5344) | Keep | — |
| Core library (5344) | Reduce to the contract | `Docs/Spec/Reference/core-library.md` and Core sources |
| FFI, Browser, Terminal (5461–6206) | Keep the boundary laws; cut API listings | Core sources |
| Formatting … Command registry (6206–7553) | **Move out** (tool and CLI behavior, not language) | `Docs/Spec/Reference/cli.md` and the CLI command registry |
| Editions … Programmable builds (7553–8511) | Keep the editions and compatibility contract; move toolchain and Jetpack mechanics | Jetpack registries and `Reference/` pages |
| Deliberately absent (8511) | Keep | — |

Also remove ratification-history identifiers inside spec prose and keep the rule
text. For example, line 2873 and `Reference/core-library.md:378` carry them.

### 4.3 Cutover hazards

- **Line-number citations into `spec.md`:** 13 `spec.md:<line>` cites in 11 source
  files, and 14 cites by section name (awk over `Compiler`, `Core`, `Tools` and
  `Jetpack`). Examples: `JetCLI/Source/CLI/BuildWeb.jet:73` and `Host.jet:1370`
  (`spec.md:4675`); `JetDriver/Source/Driver/Identity.jet:1120`; `Pipeline.jet:8996`.
  The trim shifts every line, so all 13 must become section-name cites in the same
  batch.
- The source audit's own `spec.md:N` cites will go stale. Audits are dated
  findings, so leave them as they are.

### 4.4 Design of the CI guard

- **Location and language:** `Tools/Agent/SpecBlocks.jet`, written in Jet. AGENTS
  requires repo tooling in Jet; the guard needs no Gauntlet twin because it is
  Jet-native.
- **Extraction:**
  - Reuse the existing fence scanner. Do not write a third one:
    `Tools/Agent/CoreCoverage.jet:1293-1371` (`fenced_programs`) and
    `Compiler/JetCLI/Source/CLI/Doctests.jet` already extract ```jet fences.
  - Factor one shared extractor into a library both can import (I8: one
    mechanism).
- **Info-string contract:**
  - `jet` must parse.
  - `jet ignore` is skipped and counted. The count is reported, and an increase
    fails, so nobody can silence the guard by tagging blocks.
  - `text`, `ebnf`, `json` and `toml` are not Jet.
  - A future `jet error E####` tag would assert the diagnostic code. Leave it out
    of this card: c4 says "parses".
- **Parse unit:** a block must parse as a whole file. Fragments are wrapped by one
  fixed rule (statement fragments go inside `run: fn() { … }`), tried in that order.
  The guard records which wrapper each block needed, so a block cannot pass by
  accident.
- **Parser:** the in-process `jet_parser` API of the admitted compiler, parse only.
  Do not shell out to `jet check`: that is a heavy job, and it would need full
  programs and type context.
- **No silent skips:** a missing `spec.md`, zero blocks found, or a parser that
  fails to load all **fail** the guard (AGENTS: "a proof that silently skips proves
  nothing").
- **Wiring:** `Tools/Agent/LaneCheck.jet` (fast) and `Tools/Agent/VerifyFull.jet`.
- **Self-test:** a `#Test` with one valid block, one invalid block, one `jet ignore`
  block, and one fragment.
- **Proof for c4:** the guard exits 0 on the trimmed `spec.md`, and exits non-zero
  on a scratch copy with one corrupted block. Record both runs.

---

## 5. Criteria and sequencing changes to put to the owner

1. **c2:** gate it on the §1.6 ballots Q1–Q5. Add the `POLICY_RULES` all-Tighten
   guard and the Crypto known-answer and constant-time proofs.
2. **c3:**
   - Replace "~150" with the §2.6 ratchet reaching 0 outside the built-in
     allow-list.
   - Replace the six role traits with the §2.3 set. The ratified marker decision
     binds thread-confinement, move-only and single-use.
   - Add `blockedBy` #7059 and #7056.
   - Add the §2.4 `.new` items. Items 1 and 2 are ballot-gated.
3. **c4:**
   - Split the AST collapse from the spec trim. They share no files, and the trim
     is independent.
   - Leave `UnitFamily` out.
   - Name the `#loop` origin-preservation requirement.
   - Name the `Stmt`/`Item` count guard and the codec version bump.
4. **Cross-card:** decide #7047 c3 against the #7061 `failures:` deletion before
   either builds.

**Nothing in this audit was proven by execution.** Every behavioral claim above
(for example "`#Policy(gc)` has no runtime effect") rests on code reads and
search results on `56bf8b0811d`. Each one needs the named run before any
criterion is met.
