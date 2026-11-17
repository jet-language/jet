# Adversarial review of the ten reworked ballots

Author model family: Claude Opus. Adversarial model family: GPT. Fresh agent: BallotReview.

## Posting result

Seven revised proposals are ready to post. Three must not be posted: ROWMAJOR and RUNTIME-HANDLE repeat ratified choices; ZIPCOLUMNS lacks its inherited performance/correctness evidence. All ten have complete `.r2.json` replacements with adversarial findings and an explicit verdict. Ready means a supportable design ballot, not an implemented feature or a passed implementation gate.

Recommendations changed materially for FORMAT, HOOK-POINT and BUILTIN-METHOD. Their stronger former B directions are revised A; the former A directions remain fairly described alternatives. Do not interpret those letter changes as preserving the old recommendation.

## D-GUARD-LINT1 — ready to post

**Owner direction:** r1's “tidy error, still runs” weakens the requested default ERROR. D-NO-WARNINGS1 classifies tidy findings; it does not require a new semantic error to become tidy. r2 makes an incomplete multi-case enum decision a hard error in run/dev/test/check/build, not merely shipping commands. A one-case check remains free.

**Marker:** `else #Rest ->` does not fit the tested parser. `Rest` is not a registered marker today, and post-arm handling currently recognizes Through, not arbitrary new markers. r2 proposes `else -> { body } #Rest`, explicitly requiring registration and the parser's arm-marker generalization. It does not claim that spelling works today. The same marker is used for a chain tail, not a second fallback syntax.

**Other repairs:** count distinct discriminants, not arms, so `.A | .B` is two cases while two payload patterns of `.A` remain one. Guarded cases are not full coverage. Define nested residual coverage, imported/withheld cases, the one-case-remaining case, no-else multi-case chains and excluded Bool/Int tests. Check a chain in place without caching mutable reads or joining separate early-return statements. The ordinary repair names residual cases; a separate deliberate marker repair opts into future cases. Withheld cases or an unsafe rewrite get an honest manual diagnostic, not a fake Safe fix. Removed the loose Swift analogy and unsupported prevalence framing.

**Evidence:** replayed coverage, chain and tidy probes; added isolated single-case and marker-position probes. Multi-case omissions produce E0641; both currently unsupported marker placements fail parsing. The author's e1 uses `main` rather than the required `run` and fails MissingRunFunction, so it is not proof that its example executes. Current/proposed examples are separated.

**Alternatives/recommendation:** named-only matching is a strong alternative, but does not provide the owner's free one-case checks plus deliberate inclusive fallback. Agree with repaired A, not the permissive r1 contract.

## D-FORMAT-TRAIT1 — ready to post

**Owner question q5xewxmx:** answered directly: no output argument. Both alternatives return String. Neither exposes a writer/out parameter through the public style trait.

**Found and fixed:** From and arithmetic's specialized keyed-trait support does not establish arbitrary keyed traits. A probe of that generalization fails E0003. The recommended r1 A understated new parser, conformance, bounds and dispatch work. Promoted ordinary per-style traits to A; keyed Format remains B for genuinely style-polymorphic code. The seven style families need only be implemented where supported.

Hidden return storage does not remove a String backing allocation. Replaced the universal allocation claim with the precise ratified D-EFF-FORMAT-WRITER1 closed-use lowering, and made allocating results respect D-ALLOC-FAIL1 instead of asserting Never!. Saved, aliased and other unlisted values still use ordinary String semantics. Removed blanket style inheritance for semantic distinct wrappers. Default printability and explicit numeric styles are not the same promise.

**Evidence:** replayed f1–f5: existing scalar helpers/format selectors work; arbitrary keyed traits and the unsupported Hex cases fail. Read the formatting/output/allocation decisions, Rust LowerHex and Python's return-str protocol. Rust's public writer is a real alternative elsewhere but violates this owner's requested public API; Python is not evidence for Jet's allocation behavior.

**Verdict:** disagree with r1 A; agree with revised ordinary-trait A. No performance advantage or current implementation of the new traits is claimed.

## D-ARRAY-CONVERT1 — ready to post

**Found and fixed:** a bare identifier is currently treated as shorthand more broadly than “only a real lane field”; Mat4{m} fails E0303. r2 proposes routing nonfield identifiers to conversion while keeping genuine field shorthand and existing whole-value/element construction precedence. A real collision uses explicit `.from`, not reinterpretation of a field initializer.

Removed the blanket-conversion-plus-specialized-override gap: use one coherence-checked structural From row per applicable container family under D-IMPL-COND1. Keep extent and nesting; no flattening or accidental competing implementations. Source evaluation, element order, borrowed versus consumed inputs, zero extents and prefix cleanup are explicit. Direct construction is not in-place rewriting across element widths. Fixed container storage avoids an extra container allocation, but element conversion can allocate; list reservation is not a universal one-allocator-call guarantee. Checked numeric conversion does not swallow typed failures or AllocError.

**Evidence:** explicit scalar conversions in a1 execute; whole fixed-array conversion in a2 fails E0963. Replayed the shorthand and conversion probes and checked From/coherence/copy/lane decisions.

**Alternatives/recommendation:** map is strong for arbitrary transforms and per-element recovery, not a straw man. Removed the source-named conversion family because it duplicates ratified From. Agree with corrected direct-brace A.

## D-ARRAY-MATRESIZE1 — ready to post

**Owner direction retained:** `Matrix<2,8>{grown}` is direct; no `.from_array`/`.to_array` chain, and explicit copying uses `.copy()`, not `~`.

**Found and fixed:** scalar exactness law does not itself forbid discarded matrix cells. r2 labels no-silent-crop as the proposed matrix policy and explicitly amends the old implicit-crop answer. Same shape preserves coordinates; equal total count reshapes in logical column order; growth on both axes uses identity fill; other discarding requires an explicit source window. That precedence settles equal-count mixed shrink/grow shapes instead of leaving two rules in conflict.

Removed “free reshape” and implicit physical-reuse claims. A borrowed source still needs owned-result construction, and layout changes may reorder cells. Construct directly in final storage, without pretending byte moves disappear. Settled zero/overflow extents, numeric zero/one, nonnumeric fill, move-only fill callbacks, labeled nondefault traversal and ordinary conversion/allocation cleanup. Removed ambiguous `.Identity` fill shorthand and alternatives that silently cropped after A forbade it.

**Evidence:** current small-matrix helpers execute; Mat4{m} fails E0303. Generic Matrix availability fails E0119 on this published compiler. Read the related shape, indexing, window, From, copy and layout decisions; the generic examples are explicitly proposals.

**Alternatives/recommendation:** named resize/reshape and always-labeled policy are real clarity alternatives, not slower by assertion. Agree with corrected A because it supplies the owner's direct constructor with deterministic defaults.

## D-ARRAY-ROWMAJOR1 — blocked: do not post

**Decisive finding:** D-ARRAY-MATOPS1 already ratifies `Matrix<R,C,T=Float,Layout=.ColumnMajor>`, optional row layout, views and conversions. The owner's q2ihn3ot is already answered. Columns-only and mandatory-layout alternatives would reopen settled law, so r2 removes them and records the existing implementation contract rather than presenting a fake choice.

**Other repairs:** missing `from_rows` is not proof that row input is impossible; manual argument reordering is possible. Generic source does not guarantee shared machine code or negligible specialization size. Fixed inline matrices cannot simply adopt arbitrary list allocations. Compatible transpose/layout cases can avoid a permutation, but ownership, strides, representation and external ABI still matter; no universal zero-copy or shader-layout promise remains.

**Evidence:** the current helper probe fails E0311 for Mat3.from_rows. Read the ratified matrix/index decisions and Eigen's storage-order documentation, including its workload-dependent cost discussion.

**Recommendation/verdict:** agree with optional layout. Disagree with posting a duplicate vote. The single retained A describes settled work, not an uploadable ballot.

## D-ARRAY-ZIPCOLUMNS1 — blocked: do not post

**Found and fixed:** the supposed current Columns success is unavailable on gate-p275: the new availability probe fails E0119. All join/split examples are now marked proposed. Split returns count plus fields, including for zero-field structs. Owning mismatch returns every transferred list owner and bounded metadata; borrowing mismatch does not fabricate ownership or dangling references. Ownership transfers at the call boundary, not magically after validation.

Sharing uses ordinary copy-on-write. Unique last-use input has the no-element-rebuild fast path; shared input may detach on later mutation. There is no silent eager copy, forced uniqueness or new copy API beside `.copy()`. Split returns current buffers after growth/detachment, not a promise of historical allocations. Failed growth preserves logical values/length, not necessarily capacity or addresses. Loans prevent invalidating borrowed views. Field matching, equal lengths, move-only cleanup and zero-field count are explicit.

**Alternatives:** borrowed-only joins are strong for read-only work, including move-only elements. An ordinary owning record of lists is a serious safe baseline; it does not require the unsafe-Rust straw man. Removed the redundant named-copy alternative.

**Why blocked:** D-SOA-SITE1 retains the paired correctness/performance gate against the automatic-column D-ACCEL1 path. No candidate implementation was available to run, and this assignment forbids builds and suites. There is no evidence for the owner's “fastest default” requirement, so the draft no longer claims it. The evidence needed is named in r2; the absence is not hidden behind a design assertion.

**Recommendation/verdict:** agree with repaired A's ownership design, not readiness or an unmeasured speed claim. Do not post until the inherited gate is supplied on an implementation.

## D-HOOK-POINT1 — ready to post

**Found and fixed:** failing to materialize event.new as a module constant does not rule out typed symbolic values. Uppercase spelling is not a sufficient reason to recommend a new function-shaped declaration. Promoted revised typed identities from r1 B to A; marked handler signatures remain B, fairly described.

The original behavior claims also confused handler return with dispatch return. DecisionHook handlers return HookDecision, dispatch returns HookOutcome, Transform changes later input, and Cancel/Fail stop later handlers. A point is not thereby an ordinary fn value. New dispatch probing exposed another important distinction: Hook.run takes payload AND a per-call fallback. Construction fallback alone is not the empty-dispatch result. A Hook constructed with 50 and run with 60 prints 60 without listeners and 23 with the tested handlers. The decision test prints `seen 4`, then `done`, with no third-handler print.

**Repair:** one typed symbolic-identity operation through the admitted kernel, canonical per declaration/import/instantiation, no build-time runtime object and no compiler match on library function names. Existing constructors keep dynamic-instance semantics. Point-identity casing is uniform across packages, not a Core privilege. Keep actual carrier types, run/emit arities, scope/order and existing effect/authority checking. Do not promise identity-specific effect rows erased by conversion to a plain carrier. Remove direct-call aliases, fn conversions and lazy fallback bodies. Compiler-stage points retain their separately ratified compile-time runner rather than becoming runtime Event allocations.

**Owner link question:** q012d3pt is already answered by D-LINK-STEP1's BuildAction and per-output settings, not the superseded Artifact signature.

**Evidence/verdict:** replayed h1/h2 and the new dispatch probe; inspected event constructors, sema call signatures and runtime dispatch; read hook/link/kernel decisions and Tapable. Disagree with r1 A; agree with revised typed-value A. New static constructors remain explicitly proposed, not claimed to compile today.

## D-RUNTIME-HANDLE1 — blocked: do not post

**Decisive finding:** ordinary carriers with private typed handles, Close/Shared, refusals and row-by-row delivery gates are already D-CORE-LAYOUT1. The provider-binding alternative adds a competing type-facts mechanism and is not an eligible new choice. r2 preserves the settled contract, not another vote.

**Other repairs:** the Request toy has a String field as well as a handle; no extra wrapper allocation does not imply total layout equals the handle. Privacy is module-scoped, not an automatic package-wide boundary. A raw Int handle in a same-module cleanup demo does not prove safe external construction, foreign-resource ABI or production cleanup. WIT owns/borrows resources across components; it is not simply a host-only precedent.

**Evidence:** replayed toy output `GET`, `freed 7`; added a copy probe that fails E0211 for the Close owner. Inspected handle machinery and the private-carrier/layout/provider decisions. No real foreign-resource or external-client ABI success is asserted.

**Recommendation/verdict:** agree with settled ordinary handles; disagree with posting this duplicate. Production porting must meet the existing per-object/tier gate, not a new owner vote.

## D-ITERATOR-FAIL1 — ready to post

**Found and fixed:** associated-error inference is real proposed machinery, not a free consequence of the current trait. Specify inference from next's failure contract, validate explicit Error, and retain ordinary failure conversions without inventing implicit unions. Infallible sources use Never; opening an iterable still happens before the infallible iter operation.

Fusion concerns None, not whether Err may resume. `.results()` maps each success/error to an item, then asks the source again on the next step; a terminal source records its own completion, while a recovering source can continue. No invented resumability capability or automatic error latch. A propagating loop makes no further next call. Only loop-owned resources are cleaned up by that ownership boundary; borrowed resources remain owned by the caller.

Removed unsupported runtime pipe-failure assertions: the process source's `?? [U8]{}` was inspected, but an injected pipe failure was not executed. Likewise do not present unrun walk/http failures as reproduced. Correcting existing panic/end behavior is intentional and is not falsely called a Safe behavior-preserving migration. Explicit `.results()` preserves deliberate error-as-data code. Rust Result items support forwarding/collection/propagation; not every consumer must manually unwrap in its body.

**Evidence:** the new malformed-UTF8 line loop prints the valid first line, then E3001 and exit 70. Explicit read_line prints the valid first line, catches the invalid-UTF8 IOError and exits 0. Read Iterator/Fused/borrow/collection decisions and current reader/walk/http/process implementations.

**Alternatives/recommendation:** explicit stepping avoids trait expansion; Result items are strong for outcome-processing pipelines. Agree with repaired propagation-first A, with new inference and migration costs stated honestly.

## D-BUILTIN-METHOD1 — ready to post

**Found and fixed:** being shipped inside the Core bundle does not establish an ordinary source declaration owning String. Current owner facts are not proof of every primitive's nominal source ownership. Identical names and bodies do not prove identical lookup, effects, failure behavior, code size or direct-call lowering. Removed these claims and promoted ordinary Core traits from r1 B to A. Genuine owner declarations followed by inherent impls remain B; scoped open extensions remain C.

A uses Prelude-imported, ordinary named trait contracts, not a privileged owner whitelist or compiler library-name routing. Structural array/list/lane impl targets need explicitly proposed registered target grammar and ordinary conditional-impl coherence; the working String probe does not establish that grammar today. Migrate every CoreSource member with its contract and delete obsolete routing. Audit other rows for source expressibility rather than declaring all remaining rows intrinsic machine operations.

Built-in/user trait ambiguity must be the same hard rule. Removed a built-in-only tidy escape and unsupported qualified-call fix. Existing silent collisions require an explicit migration choice, not a false Safe claim. Preserve bound-method and String ownership laws. Public helper traits are a real API cost, mitigated by coherent grouping, Prelude imports and stable documentation, not claimed to vanish.

**Evidence:** replayed bm1–bm4. Ordinary String trait methods work; plain inherent String impl fails E0301; user-type collisions produce duplicate/ambiguous diagnostics; the tested built-in collision silently selects the built-in instead. Inspected owner registration and CoreSource dispatch; read extension/kernel/source-authority/text/bound-method decisions and Rust's real owner/orphan rules.

**Recommendation/verdict:** disagree with r1 A's unsupported ownership premise; agree with revised ordinary-trait A. No identical-binary or faster-call claim remains.

## Evidence and hand-off limits

Read repository AGENTS.md first, the ballot skill and its references, orchestration review mechanics, BRIEF, owner questions and the author's supporting material. Related Tower decisions were read only. External references were read from their official documentation/repositories; unsupported analogies were removed rather than treated as proof.

All execution evidence above comes from single-file published gate-p275 probes, under `nice -n 19` with JET_TOOLCHAIN_ROOT set and LD_LIBRARY_PATH unset. Added probe sources remain under `probes/`; they are review evidence, not production implementation. No build, suite, lint, formatter, git write, repository write or Tower mutation was run. Main owns final schema/import checks and any permitted implementation verification.

Do not post the r1 files or carry forward superseded claims from answers.md/notes.md: especially GUARD's permissive tidy behavior, FORMAT's unsupported allocation promises, MATRESIZE's implicit-crop answer and HOOK's function/cancellation/fallback claims. This review did not mutate the owner-question records or Tower state. Do not upload ROWMAJOR or RUNTIME-HANDLE as fresh votes, or ZIPCOLUMNS as a proven fastest-default recommendation.
