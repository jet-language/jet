# Adversarial review: lazy precompiled Core interfaces, revision 3 (2026-11-15)

This is the second adversarial review of `Docs/proposals/2026-11-15-lazy-precompiled-core-interfaces.md`, the design note for Tower card #6328 c22. Review 1 (`Docs/audits/2026-11-15-lazy-core-interfaces-review.md`) attacked revision 2 and listed gaps G1 to G28. Revision 3 (r3, the PrecompiledCore-4 patch) rewrites the note and maps every gap to an answer in its section 12. This review does four things:

- re-attacks each r3 claim under the AGENTS.md "Adversarial design review" contract;
- checks whether each claim that failed in review 1 is now engineered away, or honestly bounded with its cost and mitigation;
- checks whether each gap answer is real;
- checks whether the author's measurement script (`measure.py`), trace splitter (`split.mjs`) and saved traces support the note's tables.

## Scope and method

- **Code base:** dev `8fb675da5c`. It is an ancestor of the mirror head (`e481784fb8`), and it includes c11c r2 (`b465a2fa62`), c11d (`0987161e75`), c11c phase 2 (`e2f8e873bd`) and the deletion of the sema body memo (`fbfc7a69d7`). Every code reference below was read at that tree.
- **Measurements:** the author's directory named in the note's `tests` file. It holds `measure.py`, `split.mjs`, the programs, five function-detail traces, the hello trace and two strace logs.
- **jet under test:** the jet the note measured: the gate-p150 batch jet at `$JET_STATE_ROOT/gate-p150/tree/Compiler/JetCli/.jet/build/jet`, built at 00:23 on 2026-11-15. That is before `9ae575e506`, c11c, c11d and c43. Its `Core/Net/Tls.jet` and `Core/Net/Net.jet` differ from dev `8fb675da5c`.
- **What I ran.** Nothing was built or tested. Two cheap probes ran under a 4 GB memory scope and were deleted afterwards:
  - an A/B timing of `jet run hello.jet`, outside any checkout, with and without `JET_TOOLCHAIN_ROOT` naming the p150 tree (11 runs per arm, two rounds);
  - a Python timing of reading and comparing the p150 tree's Core files.

  The traces were parsed with Python.
- **Missing context counts as a failed claim,** as the contract says.

## Verdicts at a glance

| # | Claim (r3) | Verdict |
|---|---|---|
| R0 | A, then the required step B, delivers c22, claimed on a census (framing, 4, 10) | holds only within stated limits |
| R1 | Baseline on p150 (section 1) | holds only within stated limits |
| R2 | Interface content, full spans, name index, impl index (3.1) | holds only within stated limits |
| R3 | `core.interface` runtime image part, 32-bit words (3.2) | holds only within stated limits |
| R4 | Load cost is inside the load spans. B bodies remap type and symbol fields, "the other fields are copied" (3.3) | fails |
| R5 | Size: about 11 MB for A, about 24 MB for B, with limits of 16, 32 and 48 MB (3.4) | holds only within stated limits for A; fails for B |
| R6 | Core text stays embedded, and rendering a diagnostic is not scanning (3.5) | holds only within stated limits (needs a ruling) |
| R7 | Step P: generated rows for the compiler-owned Prelude texts (3.6, P) | holds only within stated limits |
| R8 | Nested compiles carry the part (3.7) | holds only within stated limits |
| R9 | The leftover list is complete (section 4, G15) | fails |
| R10 | B lands whatever its speed, and regressions go to the owner (section 4) | holds only within stated limits |
| R11 | Instance facts are instantiation, not a re-check (4, 11) | holds only within stated limits |
| R12 | T1: a tree is used only when named, so 0 calls inside a checkout (5, T) | holds only within stated limits |
| R13 | A named matching tree loads the built-in interface, with a match test of at most 12 ms (5) | fails |
| R14 | Edited trees keep today's lazy text path, at today's cost (5) | holds only within stated limits |
| R15 | hello budget: at most 1 ms of Core and Prelude CPU, timed soundly (6.1) | fails |
| R16 | Never-slower definition and RSS targets (6.1d, 6.2) | holds only within stated limits |
| R17 | `coreInterfaceSha256` and a digest check (7.1) | holds |
| R18 | Drift guards: round trip, against the text path, bodies (7.2) | holds only within stated limits |
| R19 | Reach differential and impl index (7.3) | holds only within stated limits |
| R20 | One meaning across tiers, comptime and diagnostics (section 8) | holds only within stated limits |
| R21 | Alternatives (section 9) | holds only within stated limits |
| R22 | Landing steps and their proofs (section 10) | fails (two proofs are vacuous) |

## 1. Measurement audit: do `measure.py` and the traces support the tables?

### 1.1 What reproduces

The kept-body counters in 1.4 reproduce exactly from the saved traces. I parsed `sema.core_bound.*`:

| Program | kept | generic | function value | lowered | comptime | unlisted | intrinsic |
|---|---|---|---|---|---|---|---|
| fread | 79 | 52 | 4 | — | — | 18 | 5 |
| Summarize | 83 | 56 | 4 | — | — | 18 | 5 |
| RunColdContext | 591 | 260 | 153 | 99 | 55 | 19 | 5 |

The `jet check` claim also reproduces: `sema.core_checked` is 292 for Summarize and 2,123 for RunColdContext, and `jet check` has no `core_bound` counters. The hello split in 1.2 matches `split.mjs` over the hello trace. For example, `register` is 7.62 ms CPU, `sema.finalize` 4.09, `load` 2.01 and `native.link` 2.98. So the counting is honest.

The traces also hold one figure the note does not cite: `sema.core_bound.kept_ns`. It is 70.5 ms for fread, 73.6 ms for Summarize and 712 ms for RunColdContext. These are traced, inclusive times, so they are not exact, but they are the right first number for step B's payoff and should be in section 4.

### 1.2 What does not support the tables

- **M1. The peak RSS figure is a running maximum, not a median.** `measure.py` reads `ru_maxrss` from `getrusage(RUSAGE_CHILDREN)`. For children that value is the largest peak of any child reaped so far, not the peak of the last run. So the "median" over 11 runs is about the maximum of the first six. My probe showed it directly: in one 11-run series the value went from 62.9 MB on run 1 to 64.0 MB on run 11.

  The 64.2 MB target in 6.1(d) comes from this statistic, and so do the 219.6 MB for fread in 6.2. So does the "page-rounding spread of 0.5 MB" that 6.2 uses as the RSS band. The band is a property of the statistic, not of jet.

  To get a per-run peak, read each child's own rusage (`os.wait4`) or `/proc/<pid>/status` VmHWM.
- **M2. Cold is cold for the binary only.** `evict` drops the jet binary's page cache and nothing else: not the dynamic loader, not libc, not the linker that `native.link` runs. That is acceptable and the note says it, but "cold" in 1.1 and 6.1(c) means "jet binary cold".
- **M3. The match-test cost is the cost of a different read.** The 8.6 ms in 1.5 is a scripted read of all 892 files under `Core/` (20.1 MB; I counted 892 files and 20,144,412 bytes in the p150 tree).

  `jet_cli_core_tree_matches` (`CoreSources.jet` 125 to 142) does something else:
  1. It reads `CoreSourceParts.tsv` through `jet_cli_toolchain_core_rows`.
  2. It reads each source row as a `String`, which validates its UTF-8.
  3. It compares each one with the embedded text.
  4. It reads each embedded asset as bytes and compares those.

  The jet function itself was never timed (R13).
- **M4. The baseline jet is older than the base the note targets.** p150 predates `9ae575e506`, c11c, c11d and c43. Its Core differs from dev's (`Tls.jet`, `Net.jet`, `Core.rows`). The counts are therefore p150 counts. That is fine for a design note, but A0 must re-measure on its own base before any gate uses the numbers.
- **M5. The 1.2 split covers 66%.** The note says so and gives A0 the job of reaching 95%. That is honest, but it is a promise, not a measurement (G2 below).
- **M6. Single-thread claims are unverified.** Every hello span is on one trace thread (tid 1). The strace logs trace only file calls, so they cannot show whether the process starts other threads. Section 6.1's timing argument depends on hello being single-threaded (R15).

## 2. Code facts checked at 8fb675da5c

| r3 statement | At 8fb675da5c |
|---|---|
| `core.jet` is the empty unit Loader adds for module `core` | true: `jet_driver_add_core_unit` (`Loader.jet` 4783) |
| Effect vocabulary is `Compiler/Prelude/Effects.jet`, 3,147 B (1.3) | incomplete. `EFFECT_CANONICAL_SOURCE` (`EffectVocabulary.jet` 41) concatenates `Compiler/Prelude/Effects.jet` (3,147 B), `Core/Effects/Effects.jet` (763 B) and `Core/Compiler/Meta.jet` (746 B). 1,509 B of Core text are parsed every compile, and 1.3 does not name them |
| The effect parse runs every compile | true: `Pipeline.jet` 4944 and 4963. The source also goes to sema as a module source (5453), and to `CompilerQueries.jet` 231 and `ScriptHelpers.jet` 258 |
| `jet_cli_runtime_part` lends a part, and panics if it is missing | true: `Host.jet` 737 to 741 |
| `JetCLICoreSealRead` reads the seal and facts lazily | true: `CoreSources.jet` 108 to 118 |
| `jet_cli_toolchain_root` searches upward from the entry | true: `Request.jet` 458 to 471. It has 14 callers in JetCli (R12) |
| The match test compares the Core rows and assets byte for byte | true: `CoreSources.jet` 125 to 142 |
| `provenance.json` records `coreFactsSha256` and `coreSealSha256`, and nothing reads them | true: `RuntimePack.jet` 537 to 540 is the only occurrence in Compiler, Tools/agent and Jetpack |
| `OWNED_TRAITS` and the diagnostic rows are generated and checked | true (pattern exists). `EFFECT_VOCABULARY_ROWS` (`EffectVocabulary.jet` 27) is already generated at compile time, but sema still parses the canonical source |
| `ir_core_method_lowered_by_owner` names the `impl Debug` hazard | true: `JetIR/Source/CoreReach.jet` 230 |
| `jet_driver_core_lowering_roots` selects roots by owner | true: `Pipeline.jet` 4113 |
| `IRProgram.functions: [FnBody]` | true: `JetIR/Source/Build.jet` 483 |
| Derive providers are parsed with `parse_source` | true: `Derives.jet` 2079 to 2080 |
| Error family and units are `#run` texts | true: `PreludeDerives.jet` 163 and `PreludeUnits.jet` 7 |
| Nested compiles copy Core sources | true: `ComptimeCompiler.jet` 137 to 138 and 271 to 272 copy both `core_sources` and `canonical_effect_source` |
| Generic instances are type substitution over the generic check | true for types: `Instances.jet` 1 to 8 and 32 to 41 |
| Core Prelude parse is counted in `parse.core_input_bytes` | **false.** `Loader.jet` 4712 to 4716 counts a `.Prelude` job as a **user** job and as `user_input_bytes`. The hello trace has `parse.core_input_bytes` = 0 while it parses `Core/Prelude/Prelude.jet` (1.26 ms) |

## 3. Claims

### R0. A, then the required step B, delivers c22, claimed on a census

**Prior art.** Rust: a dependency's crate metadata carries signatures and MIR for generic and inline items, so a downstream crate never re-parses std (rustc-dev-guide, "Libraries and metadata"). Swift: binary `.swiftmodule` files carry declarations and inlinable bodies as serialized SIL (`docs/Serialization.md`). Go: unified IR export data carries declarations and inlinable or generic bodies (the Go 1.18 dictionaries design and the unified IR proposal). All three ship bodies wherever a client needs one. r3 now does the same by requiring B, which closes review 1's C0 failure in principle.

**Attack.**

- **The claim condition counts bodies only.** c22 is claimed when the census reports `core.bodies_from_text` = 0. But c22 forbids reading, scanning and re-checking Core or Prelude source. The census does not report:
  - `prelude.text_bytes_scanned`;
  - a parse counter that includes the Core Prelude;
  - calls on a toolchain tree.

  Only hello is straced, and only for hello is `prelude.text_bytes_scanned` gated (step P says "hello and the corpus", but the census does not report it).
- **The body counter misses bodies.** It misses the Core Prelude's bodies and the compiler-owned Prelude's bodies (R9). A census can report 0 while hello still checks `Core/Prelude/Prelude.jet` bodies from text.
- **The census is `jet run` only.** That matches c22's verb. But B5's build proof covers only the 6.2 programs, and A3 (`jet check`) has no body-count target. That is acceptable if stated. r3 does not say that `jet check` and `jet build` over the corpus are outside the claim.

**Verdict: holds only within stated limits.** B being required fixes the structure. The claim condition is narrower than c22's text.

**Gaps:** H1, H2, H13.

### R1. Baseline on p150

**Attack.** The untraced medians and the trace counters are real (section 1.1 of this review). The weaknesses:

- the RSS statistic (M1);
- the binary-only cold run (M2);
- the match read is not the match test (M3);
- a pre-base jet (M4);
- table 1.3 names the effect vocabulary as one 3,147 B Prelude file, which hides 1,509 B of Core text parsed every compile. Review 1 found this, and r3 kept the wrong row.

**Verdict: holds only within stated limits.**

**Gaps:** H3, H4, H5.

### R2. Interface content, full spans, name index, impl index

**Prior art.** Clang PCM and Swift modules both keep on-disk hash tables keyed by identifier, read lazily. Swift's lookup tables are its "lazy member loading". r3's open-addressing index (2^16 slots, load factor 0.55, 0.52 MB) is the same idea, and full byte spans close review 1's span gap.

**Attack.** The impl index loads every impl of a reached nominal type. For a widely implemented type such as `String` or `List`, this is many impl rows and their method tables. That is the shape of the reach widening that sank the retired record. r3 does not count impl rows loaded for hello or fread, and does not budget them. Section 6.1(a) covers their CPU, but nothing says how many rows hello will touch. "A few hundred rows" in 3.3 is an estimate, which A0 is to count.

**Verdict: holds only within stated limits.**

**Gaps:** H6.

### R3. `core.interface` runtime image part, 32-bit words

**Prior art.** Same route as `core.seal` and `core.facts` today. GHC `.hi` files and Lean `.olean` files are separate files instead; r3's in-image part avoids a file lookup.

**Attack.** Checked in code: `jet_cli_runtime_part` exists and lends bytes, so this route closes review 1's C3 failure (no second compiler compile). Two costs are unmeasured:

- **ns per field:** "1 to 2 ns per field" is an estimate. A1 measures it, but nothing gates it.
- **Page touches:** pages of a 264 MB image are faulted on first touch. The cold numbers (28 major faults) show readahead hides most of this today, but the interface adds 11 to 35 MB of new, randomly probed pages.

**Verdict: holds only within stated limits.**

**Gaps:** H7, H8.

### R4. Load cost: B bodies remap type and symbol fields, "the other fields are copied"

**Prior art.** Swift deserializes declarations and SIL lazily, one entity per request, through a cross-reference table. Rust decodes MIR on demand per `DefId`, through `rmeta` tables with lazy positions. Both decode into compiler data structures. Neither claims the rest is a copy.

**Attack.** Checked against `JetFoundation/Source/Facts/Body.jet`. "The other fields are copied" does not describe these types.

**`NodeKind` has 98 variants.** Many carry data that is neither a type nor a symbol and has to be built:

- strings: `TraitConst(… trait_name: String, name: String)`, `TryWithContext(… file: String, … function: String)`, `UnitConvert(… scale: String, offset: String, … file: String …)`, `PoolSlot(… source_line: String)`, `MathBuiltin(type_name: String, function: String …)`;
- compile-time values: `CtLit(value: TComptimeValue)`;
- nested records: `Lambda(value: BodyLambda)`, a record of about 20 fields with lists of captures, types and strings;
- declaration ids: `Call(callee: DeclID …)`, `GlobalShared(initializer: DeclID …)`;
- lists: `[Type]`, `[BodyCallArg]` and `[Int]` on most call and literal variants.

**`FnBody` holds about 50 fields.** They include `params`, `origin_facts`, `typed_goals`, `index_proofs`, `pre`, `masked`, `fixed_tags` and `effect_scope_regions`. Each `BodyTree` has its own `TypeTable` (types, hashes, slots) and its own `SymbolKeys` table.

**Some `FnBody` fields belong to the program, not to Core.** Whole-program passes rewrite them on every compile:
- `WebPartition.jet` 120 sets `web_bucket` and `web_reason` on every function in the modules;
- `Effects/Checks.jet` 5822 to 5825 sets `effect_scope` (capped by the program's `#FX` frames), `heap`, `stack` and `frame_bound`;
- `LocalSplice.jet` 522 to 525 says the failure solve "solves its failure facts again, which may restamp it and the bodies that call it".

So a loaded Core body cannot stay a read-only lent row. It has to become an owned, mutable `FnBody` that the whole-program passes walk and restamp. That cost lands in `sema.finalize`, outside the `core.interface` spans that 3.3 says hold all the cost.

This is the record's failure mode: decoding cost more than parsing. r3 bounds it only by "B lands whatever its speed".

**Verdict: fails.** The decode description is wrong for B, and the cost of whole-program passes over loaded bodies is unaccounted for.

**Gaps:** H9, H10.

### R5. Size

**Attack.** The step A arithmetic checks out from the stated counts, for example 19,643 × 22 words × 4 B = 1.73 MB.

Step B is a floor, not an estimate. Its rows count:
- nodes at 7 words;
- statements at 5 words;
- body headers and locals at 16 words.

They omit:
- **`BodyTree.expr_types`:** one `TExprType{id, ty}` per typed node (`Checked.jet` 404). At 2 words over 695,467 nodes that is about 5.6 MB more.
- **Per-body type tables:** each body interns its own types.
- **Per-body symbol-key tables.**
- **Side tables for list, string and record payloads:** a 7-word node cannot hold `Lambda(BodyLambda)` or `Call(… [Type], [BodyCallArg])`.
- **About 50 header fields:** `FnBody` has about 50 fields, not 16 words.

The node count (695,467) comes from the record audit, not from current Core.

With `expr_types` alone, B is about 30 MB, close to the 32 MB limit. The stated mitigation (16-bit node words) does not touch the omitted columns.

**Verdict:** holds only within stated limits for A; fails for B.

**Gaps:** H11.

### R6. Core text stays embedded; rendering a diagnostic is not scanning

**Attack.** The text stays for three uses: diagnostics, the match test and the jet build. That matches the code. Whether reading embedded Core text to render a diagnostic counts as "reads Core source" under c22 is a reading of the card, and r3 asserts it. A diagnostic only reads the lines it quotes, and only on an error path. That is a defensible reading, but it is the owner's to make.

**Verdict: holds only within stated limits.** It needs a recorded ruling.

**Gaps:** H12.

### R7. Step P: generated rows for the compiler-owned Prelude texts

**Prior art.** The repo's own `OWNED_TRAITS` and diagnostic-row generators, which are checked in and have a `--check` mode. `EFFECT_VOCABULARY_ROWS` is already compile-time rows.

**Attack.**

- **The P proof "`parse.core_input_bytes` is 0 for hello (G4)" is already true at p150.** The hello trace has it at 0, because the effect parse does not go through the Loader's job loop, and the Core Prelude counts as a user job (`Loader.jet` 4712). The proof cannot fail, so it proves nothing.
- **Derive providers contradict the P proof.** Their `expand` bodies ship only in B3, so `sema_derive_builtin_providers` (`Derives.jet` 2080) still calls `parse_source(SEMA_PRELUDE_DERIVE_SOURCE)` for every corpus program that derives, until B3. P's proof says `prelude.text_bytes_scanned` is 0 "for hello and the corpus" before A1. Either P must also generate the providers' marker declarations, or the corpus half of the proof moves to B3.
- **The error family's bodies are still checked from text.** P replaces the error family's declarations. The hello trace shows the error family's bodies still checked from text: `sema.module Prelude/Errors` takes 0.19 ms CPU, and `comptime.check Err.__jet___memo_error_metadata` runs. Section 4 has no category for them.
- **The effect module itself stays.** The canonical source also reaches sema as a module source (`Pipeline.jet` 5453), which diagnostics and spans use. P must say what replaces that module source.

**Verdict: holds only within stated limits.** The approach is sound, but its proofs are vacuous or contradictory.

**Gaps:** H2, H13, H14.

### R8. Nested compiles carry the part

**Attack.** 3.7 names only `ComptimeCompiler.jet`. The canonical effect source and Core sources are also carried by the main request (`Request.jet` 1381 to 1382), compiler queries (`CompilerQueries.jet` 231 and 240) and scripts (`ScriptHelpers.jet` 257 to 258). The claim is right in spirit but does not list every request builder.

**Verdict: holds only within stated limits.**

**Gaps:** H15.

### R9. The leftover list is complete (G15)

**Attack.** Section 4 lists 17 categories with counts, a large improvement on review 1's C6. It still misses:

- **Core Prelude bodies.** `Core/Prelude/Prelude.jet` (11,256 B, 17 functions) is loaded as a `.Prelude` job, which counts as user code. The hello trace shows `sema.module Core/Prelude/Prelude` with 78 items in `sema.check`, and 1 body checked in `sema.bodies` at 0.40 ms CPU. These are Core bodies re-checked from text on every compile. None of the 17 categories names them, and `core.bodies_from_text` would not count them unless A0 adds a category.
- **Compiler-owned Prelude bodies.** `Prelude/Errors` (32 items) has bodies and comptime items checked from text (R7).
- **Per-program body facts.** The fields that whole-program passes recompute (R4) mean every loaded Core body is still walked and restamped. That is not "from text", but it is the work B was meant to remove. It needs a row with a count.

**Verdict: fails** (incomplete; missing context counts as failure).

**Gaps:** H2, H10.

### R10. B lands whatever its speed; regressions go to the owner

**Attack.** c37 says never slower. r3 states the conflict openly, ranks conforming forms, and reports any slower program with its numbers. Under AGENTS.md this is an honest bound, not cheating. But it pre-decides a c22-against-c37 trade that belongs to the owner, and nothing in section 10 makes a B step wait for that ruling.

**Verdict: holds only within stated limits.**

**Gaps:** H16.

### R11. Instance facts are instantiation, not a re-check

**Prior art.** Rust's monomorphization collector instantiates MIR without re-type-checking. Swift specializes serialized SIL. Both rely on facts fixed at the generic check. For effects, Koka's row-polymorphic effects instantiate effect rows by substitution.

**Attack.** For types the code supports r3: `Instances.jet` substitutes the generic body's type table row for row, so the IR builder never re-checks. For failure and effect facts it is not that simple. The failure solve runs whole-program and "may restamp" bodies (`LocalSplice.jet` 522 to 525). A generic body whose trait-method calls have failure or effect facts that depend on the impl chosen for `T` gets its instance facts from that solve, not from the generic check alone. r3 names this risk in section 11 and makes B1 list the per-instance rules, which is the right bound. But it still calls instance facts "instantiation", and it gives no evidence that the solve is a pure substitution.

**Verdict: holds only within stated limits.**

**Gaps:** H10, H17.

### R12. T1: a tree is used only when named (ruled)

Main ruled T1, so this review checks only its consequences.

**Attack.**

- **The search has 14 callers in JetCli, and r3 names none:**
  - `Budget.jet` 419;
  - `BuildCache.jet` 176;
  - `CoreSources.jet` 78;
  - `EditorGrammars.jet` 82;
  - `Host.jet` 3264;
  - `Inspect.jet` 656;
  - `Request.jet` 438, 1003, 1227, 1310, 2033, 2052 and 2073;
  - `ScriptCache.jet` 105.

  `jet inspect` and `jet devtools grammars` take the tree from the current directory and fail with E2105 when it is missing. Under T1 they would fail inside a checkout unless the variable is set.
- **Recognizing the named cases needs the search.** T1 lists "a Core test run, or a compiler package build" as naming a tree. Today both are recognized through that same upward search (`jet_cli_is_core_library`, `jet_cli_core_test_path`). r3 does not say how they are recognized without it, or how the recognition avoids touching `Core/` for an ordinary entry.
- **The claim that agents and builds name the tree holds.** `Land.jet`, `Suite.jet`, `VerifyFull.jet`, `LaneCheck.jet`, `CompilerRegistries.jet`, `CoreModuleCheck.jet`, `Chain.jet` and the orchestrator scripts all set `JET_TOOLCHAIN_ROOT`. That is exactly why R13 matters.

**Verdict: holds only within stated limits.** The strace clause can be met. The migration of the other callers is missing.

**Gaps:** H18.

### R13. A named matching tree loads the built-in interface; the match test is at most 12 ms

**Prior art.**

- Cargo decides freshness from a fingerprint of file mtimes and sizes.
- ccache "direct mode" hashes inputs only when the manifest's stat data misses.
- Go's build cache keys on content hashes, but caches the file hashes by stat data.

None reads 19 MB of sources per compile to decide that nothing changed.

**Attack.**

- **Today's cost of a named matching tree is small.** My probe ran p150 hello outside any checkout, 11 runs per arm, two rounds, at load average about 10:

  | | Built-in Core | Named tree |
  |---|---|---|
  | Wall median, round 1 | 36.0 ms | 38.7 ms |
  | Wall median, round 2 | 39.3 ms | 40.3 ms |

  The author's `st-matching.txt` shows why the cost is small: 268 calls on `Core/`, every one a `statx`, and no file content read. The match test today runs only when a deferred body is first needed, and hello needs none.
- **r3 moves the full match onto every compile that names a tree,** "before any Core declaration loads". The read alone is 8.6 ms by the note's own number. My Python probe on the p150 tree gave:
  - reading all 892 files: median 10.2 ms (minimum 9.1);
  - reading, UTF-8 decoding and comparing the 272 `.jet` files: median 14.7 ms (minimum 12.9).

  The jet function does the second kind of work, plus the asset reads. `[INFERENCE]` it will exceed 12 ms on this machine. r3 never timed the function itself (M3).
- **The added cost lands on every compile the test and agent tooling make,** because they all name the tree (R12). Every golden, ui case, lane check and agent tool would pay about 10 to 15 ms more per compile, roughly 30% to 40% of hello.
- **6.2's never-slower rule cannot see it.** 6.2 is measured "outside any checkout" with no tree named.
- **This is a c37 regression created by the design,** not a property of edited trees.

**Verdict: fails.** The limit is likely not met, the regression falls outside 6.2's scope, and a cheaper decision procedure is not considered.

**Gaps:** H19, H20.

### R14. Edited trees keep today's lazy text path

**Attack.** It is cost-neutral by construction. A2 re-measures hello and Summarize in an edited tree, which answers review 1's C8. Three costs remain:

- There are two loading paths for good (I8). The guards that keep them equal run only on built-in Core (section 11).
- A lane that edits Core runs its whole test suite on the text path. That lane never exercises the interface path for the Core it changed, and only the build's guards on built-in Core catch a divergence.
- The match test (R13) is paid first, before falling back to text, so an edited tree pays the read up to the first difference on top of today's cost.

**Verdict: holds only within stated limits.**

**Gaps:** H19, H21.

### R15. hello budget: at most 1 ms of Core and Prelude CPU, timed soundly

**Attack.**

- **The floor today is more than 1.36 ms.** The hello trace attributes:
  - parse of `Core/Prelude/Prelude.jet`: 1.26 ms;
  - `core.jet`: 0.10 ms;
  - sema per-module work for `Core/Prelude/Prelude`, `core`, `Prelude/Errors` and `Prelude/Effects.jet`: 0.13 ms in `sema.check` and 0.69 ms in `sema.bodies` (0.40 + 0.06 + 0.16 + 0.07).

  That is at least 2.2 ms before the effect parse and the Prelude share of `register` (7.62 ms), which r3 says are unmeasured.

  Step A removes parsing. It does not remove the per-module passes over the four Core and Prelude modules, which run at a fixed cost per module per pass (about 0.03 to 0.06 ms each in this trace). Nothing in r3 shows that 1 ms is reachable, or which step reaches it. The budget may be right as a target, but r3 has no evidence that A2 can meet it.
- **The timing method measures the wrong clock.** r3 times per-row loads with a monotonic clock through the vDSO, and argues that "hello is single-threaded, so that clock is its CPU clock". Wall time includes preemption, page faults and I/O waits. On a machine at load 9 to 14, which section 1 reports, preemption inside a 1 ms window is common.

  The budget is defined in thread CPU. On Linux x86-64, `CLOCK_THREAD_CPUTIME_ID` is not served by the vDSO, which is presumably why the monotonic clock was chosen. Spans already carry per-thread `cpu_ns`. A row loader that runs inside its own span gets CPU directly. The note does not say why the per-row loads cannot be grouped into spans.
- **Single-threadedness is asserted, not shown (M6).**
- **(e) counts distinct bytes, but cost scales with pages.** `interface.bytes_touched` counts distinct bytes. Lookups probe a 0.52 MB hash index and rows spread across 11 MB, so a few hundred lookups can touch a few hundred distinct 4 KB pages: 1 to 2 MB of RSS and page-ins, while touching under 64 KB of distinct bytes. Pages are the right unit for RSS and faults.

**Verdict: fails.** The budget is not shown to be reachable, and the timing method does not measure what the budget defines.

**Gaps:** H22, H23, H24.

### R16. Never-slower definition and RSS targets

**Attack.** The definition is concrete:
- seven programs;
- `jet check` and `jet run`;
- a median of 11, plus one cold run;
- a noise band of 3% and 2 ms.

That is better than review 1's C11. The weaknesses:

- **The RSS rules disagree.** 6.1(d) says hello's peak RSS does not rise. 6.2 allows +0.5 MB. Both rest on the running-max statistic (M1).
- **The 2 ms band on hello is 6 to 8% of its time.** A 1 ms budget overrun is invisible to 6.2, so only 6.1's span gate can catch it.
- **Named-tree compiles are excluded (R13).**
- **One cold run gives no distribution.**

**Verdict: holds only within stated limits.**

**Gaps:** H4, H19, H25.

### R17. `coreInterfaceSha256` and a digest check

**Attack.** Checked: the digest keys exist and nothing reads them. Adding a check in the runtime step that recomputes each part's digest from the image is real and cheap.

**Verdict: holds.**

### R18. Drift guards

**Attack.**

- **Round trip:** comparing interface rows with the text path's registration rows is not circular, which closes review 1's C7.
- **Body guard:** loaded body equals text-checked body, row for row. That holds only if "row for row" excludes the per-program fields (R4). Otherwise the guard either fails on every program that restamps them, or it compares bodies before the whole-program passes, which is not what the IR consumes.
- **Scope:** the guards run on built-in Core only. An edited tree runs no guard (R14).

**Verdict: holds only within stated limits.**

**Gaps:** H10, H21.

### R19. Reach differential and impl index

**Attack.** A differential guard between text reach and interface reach over the corpus, plus the `{value:Debug}` case, is a real answer to review 1's C12. `ir_core_method_lowered_by_owner` exists and names the hazard. The differential compares reached sets, but the cost of the wider impl loading is not measured (R2).

**Verdict: holds only within stated limits.**

**Gaps:** H6.

### R20. One meaning across tiers, comptime and diagnostics

**Attack.**

- **Tiers:** the tier proof covers `jet run` at O0, `jet build -O1` and `jet build -O2`.
- **Web and Wasm:** I9 says "web where applicable". `FnBody` carries `web_bucket`, `web_marker` and `endpoint`, and `WebPartition.jet` assigns them per program. Section 8 does not mention the web or Wasm tier.
- **Comptime:** the comptime proof is concrete.
- **Diagnostics:** byte-identical diagnostics on the ui corpus is the right bar.

**Verdict: holds only within stated limits.**

**Gaps:** H26.

### R21. Alternatives

**Attack.** It is now fair, and gives costs for separately compiled generics, machine-code instances, mapped object graphs and caches. It misses:
- a stat-based or size-based way to decide that a tree matches (Cargo, ccache), which bears directly on R13;
- grouping row loads into CPU-timed spans, as an alternative to the vDSO timing (R15).

**Verdict: holds only within stated limits.**

**Gaps:** H20.

### R22. Landing steps and their proofs

**Attack.**

- **A2's proof "`parse.core_input_bytes` is 0 for hello" is already true at p150, so it cannot fail.** The counter excludes the Core Prelude (`Loader.jet` 4712) and the effect parse. The proof that matters is that `Core/Prelude/Prelude.jet` is not parsed.
- **P's corpus proof contradicts B3 (R7).**
- **T's strace filter counts the right thing.** It awks for the tree's `Core` and `Compiler/Prelude` prefixes in `openat`, `statx`, `getdents64`, `stat` and `readlink`. The upward search's `statx` of `<checkout>/Core` matches that prefix, so a lingering search would be caught. Good.
- **B5 covers `jet build` for the 6.2 programs only.**
- **A0 commits to 95% attribution and the census.** That is real, but it gates later steps, not A0 itself.

**Verdict: fails** while two proofs are vacuous or contradictory.

**Gaps:** H2, H13, H14.

## 4. Review 1 gaps: are the answers real?

| Gap | r3 answer | Status |
|---|---|---|
| G1 strace inside a checkout | T1, step T | real for strace; the migration of the other callers is missing (H18) |
| G2 split reaches 95% | A0 adds spans | deferred: 66% today, and A0 commits to 95% |
| G3 every input named | 1.3 | partial: `core.jet` is right; the effect vocabulary hides 1,509 B of Core text (H3) |
| G4 effect vocabulary | P | partial: the approach is real, the proof is vacuous (H13), the module source is unaddressed (H14) |
| G5 compiler-owned Prelude | P | partial: derive providers conflict with B3; Errors bodies are uncounted (H2, H13) |
| G6 nested compiles | 3.7 | partial: only `ComptimeCompiler` is named (H15) |
| G7 defaults | B4 | real |
| G8 embedding route | runtime part | real |
| G9 decode cost | 3.3, 6.1 | not real for B ("other fields copied"; R4); partial for A |
| G10 index and spans | 3.1, 8 | real |
| G11 size | 3.4 | real for A; not real for B (H11) |
| G12 binary limit | 48 MB | real |
| G13 sound hello budget | 6.1 | not real: wrong clock, no evidence the budget is reachable (H22, H23) |
| G14 cold versus warm | 1.1, 6.1 | real, with binary-only cold stated (M2) |
| G15 complete leftover list | section 4 | not real: misses Core Prelude and Errors bodies, and the per-program facts (H2, H10) |
| G16 census | section 10 | partial: counts bodies only (H1) |
| G17 hello clause owner | 6.3, ruled | real |
| G18 provenance | 7.1 | real |
| G19 non-circular guard | 7.2 | real; the body guard's field scope is missing (H10) |
| G20 tree-mode cost | 5 | real for edited trees |
| G21 matching tree | 5 | not real: the cost regresses every named-tree compile (H19, H20) |
| G22 tier parity | 8 | partial: no web or Wasm tier (H26) |
| G23 comptime parity | 8 | real |
| G24 RSS target | 6.1, 6.2 | partial: the statistic is wrong and the rules disagree (H4, H25) |
| G25 B required | 4 | real |
| G26 B metric | 6.2 | real, apart from the owner ruling for a slower B (H16) |
| G27 reach differential | 7.3 | real |
| G28 alternatives | 9 | real, minus the tree-match alternatives (H20) |

## 5. Remaining gaps (criterion text)

- **H1.** The c22 claim condition covers every clause of c22 on the census. For hello, the 6.2 programs, and every example and gauntlet program under `jet run` on built-in Core, the census reports 0 for each of:
  - `core.bodies_from_text`;
  - `prelude.text_bytes_scanned`;
  - a Core parse counter that includes `Core/Prelude/Prelude.jet` and the canonical effect source;
  - calls on any toolchain tree.
- **H2.** `core.bodies_from_text` has categories for Core Prelude bodies (loaded as a `.Prelude` job) and for compiler-owned Prelude bodies (`Prelude/Errors` and the effect module). Section 4 lists both with counts from a trace, and a step of B names each as removed.
- **H3.** Table 1.3 names the canonical effect source as the concatenation of `Compiler/Prelude/Effects.jet`, `Core/Effects/Effects.jet` and `Core/Compiler/Meta.jet` (`EffectVocabulary.jet` 41), with each file's byte count.
- **H4.** Every peak-RSS figure and gate uses each run's own peak (`wait4` rusage or VmHWM per child), not `RUSAGE_CHILDREN`'s running maximum. 64.2 MB, 219.6 MB and the 0.5 MB band are re-measured that way.
- **H5.** A0 re-measures section 1 on a jet built from its own base, including c11c, c11d and c43, before any budget or gate uses the numbers.
- **H6.** A0 counts the interface rows hello and fread load, by kind (declarations, impl rows, method tables, strings). A2's proof gives those counts and their CPU next to the 6.1 budget.
- **H7.** A1 gates ns per field read. The gate value is set before A1 lands, and A2 does not start unless it is met.
- **H8.** A2 reports major and minor faults and resident interface pages for hello, cold and warm, against the jet before.
- **H9.** Before B1, 3.3 lists every `FnBody`, `BodyTree` and `NodeKind` field kind and how each is materialized: strings, `TComptimeValue`, `BodyLambda`, `DeclID`, per-body `TypeTable` and `SymbolKeys`. A B1 prototype measures materialization ns per body on the 6.2 programs, against checking the same body from text.
- **H10.** Before B1, the note lists every `FnBody` field that a whole-program pass writes per compile: `web_bucket`, `web_reason`, `effect_scope`, `heap`, `stack`, `frame_bound`, and the failure-solve restamps. For each it says whether the field is shipped, cleared, or recomputed. The cost of those passes over loaded Core bodies is timed on the 6.2 programs, and the body guard (7.2) compares only the shipped fields.
- **H11.** 3.4's step B estimate counts `expr_types`, per-body type and symbol tables, the side tables for `NodeKind` payloads, and all `FnBody` header fields, from a census of current Core's checked bodies, not the record audit. The 32 MB limit is re-justified against that number.
- **H12.** A Main or owner ruling records whether rendering a diagnostic from embedded Core text satisfies c22's "reads no Core source", before A2 claims the clause.
- **H13.** Every proof in section 10 can fail on today's jet. A2's and P's "`parse.core_input_bytes` is 0 for hello" is replaced by a counter that includes the Core Prelude and the canonical effect source, which is non-zero today. P's corpus claim for `prelude.text_bytes_scanned` either covers the derive-provider declarations in P, or moves to B3.
- **H14.** P states what replaces the canonical effect module's source in sema (`Pipeline.jet` 5453) and in compiler queries and scripts, and shows that diagnostics pointing into the effect vocabulary stay byte-identical.
- **H15.** 3.7 lists every request builder that carries `core_sources` or `canonical_effect_source`, and A2 proves that each carries the part instead: the main request (`Request.jet`), comptime (`ComptimeCompiler.jet`, both copies), compiler queries (`CompilerQueries.jet`) and scripts (`ScriptHelpers.jet`).
- **H16.** If a B step makes any 6.2 program slower, it does not merge until the owner has ruled on the c22-against-c37 conflict with that step's numbers.
- **H17.** B1 shows, on every generic Core function the corpus instantiates, that the instance's effect and failure facts after the whole-program solve equal today's per-program result. It names every case that needs more than substitution.
- **H18.** Step T lists every caller of `jet_cli_toolchain_root` and its rule under T1. It states how a Core test run and a compiler package build are recognized without an upward search for `Core/`. It keeps `jet inspect` and `jet devtools grammars` working inside a checkout, or records their new requirement.
- **H19.** 6.2's never-slower set includes the same programs with `JET_TOOLCHAIN_ROOT` naming a matching tree and an edited tree. A2 does not land if a named matching tree makes hello slower than today's named-tree hello beyond the noise band.
- **H20.** The matching-tree decision is timed as the jet function (`jet_cli_core_tree_matches` or its successor), not as a scripted read. If it exceeds the limit, the note evaluates a cheaper decision (stat data or sizes against embedded rows, or deferring the decision until a body is needed as today) against the c22 "no runtime cache" rule, and records which is chosen.
- **H21.** The text-path guard (7.2) and the reach differential (7.3) also run in a lane that edits Core, or the note records that edited-tree lanes never exercise the interface path for their changed Core and names the gate that catches it.
- **H22.** 6.1(a) is timed in thread CPU: the interface load runs inside spans that report `cpu_ns`, or another CPU-time source. A wall clock is not allowed. A0 shows that hello is single-threaded during those spans, or the budget sums CPU over threads.
- **H23.** Before A2, A0 attributes today's Core- and Prelude-attributable hello CPU in full: parse, per-module sema passes for each Core and Prelude module, the effect parse, and the Prelude share of `register`. The note names the step that brings the sum under 1 ms, or Main rules a different budget.
- **H24.** 6.1(e) counts distinct 4 KB pages of `core.interface` touched, with a page limit, in addition to or instead of distinct bytes.
- **H25.** 6.1(d) and 6.2 state one RSS rule for hello: either "no rise" or "+0.5 MB", not both.
- **H26.** Section 8 covers the web and Wasm tier: a program that opts into the web partition produces the same partition (`web_bucket` and `web_reason`) and output with the interface path on, or the note states that the tier is not applicable and why.

## Sources

- **Repository** at dev `8fb675da5c`:
  - `Compiler/JetCli/Source/Cli/CoreSources.jet`, `Request.jet`, `Host.jet`, `Budget.jet`, `BuildCache.jet`, `EditorGrammars.jet`, `Inspect.jet`, `ScriptCache.jet`;
  - `Compiler/JetDriver/Source/Driver/Loader.jet`, `Pipeline.jet`, `ComptimeCompiler.jet`, `CompilerQueries.jet`, `ScriptHelpers.jet`;
  - `Compiler/JetFoundation/Source/Facts/Body.jet`, `Checked.jet`, `Registry/EffectVocabulary.jet`;
  - `Compiler/JetSema/Source/Sema/Instances.jet`, `WebPartition.jet`, `Effects/Checks.jet`, `Comptime/LocalSplice.jet`, `Registration/Derives.jet`, `PreludeDerives.jet`, `PreludeUnits.jet`;
  - `Compiler/JetIR/Source/Build.jet`, `CoreReach.jet`;
  - `Compiler/Toolchain/RuntimePack.jet`;
  - `Tools/agent/` (Land, Suite, VerifyFull, LaneCheck, CompilerRegistries, CoreModuleCheck, Chain).
- **Author's measurements:** `measure.py`, `split.mjs`, the five function-detail traces, the hello trace, `st-matching.txt` and `st-inW.txt`.
- **Prior art:**
  - Swift `docs/Serialization.md`;
  - rustc-dev-guide, "Libraries and metadata";
  - Go 1.18 generics implementation design (dictionaries and GC shape stenciling);
  - the Cargo book's "Build cache" (fingerprints);
  - ccache manual ("direct mode");
  - Koka documentation on effect rows.
