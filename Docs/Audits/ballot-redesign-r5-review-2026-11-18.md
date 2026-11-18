# Adversarial review of r5 ballots — 2026-11-18

Author model family: Claude Opus. Adversarial model family: GPT. Fresh reviewer: BallotReview2 (runtime BallotReview2-2). Reviewed the four r5 payloads, their notes and executable claims; repaired payloads are `<ID>.r6.json` beside this audit. No Tower, git or repository writes.

## Verdicts

- D-CALLBACK-ONCE1 — **holds within stated limits**.
- D-FNVAL-COPY1 — **holds within stated limits**.
- D-SERVICE-CURRENT1 — **holds within stated limits**.
- D-ARRAY-ZIPCOLUMNS1 — **fails**: the strict pre-ratification performance gate remains unsatisfied (#6177 c17).

These supersede my earlier binary Not ready reports. Per Main's clarified scale, an unbuilt feature is not automatically a failed design claim. LIMIT means the r6 ballot states an implementation limit, its cost and mitigation; the build card's first criterion must prove it. PREREQ means an identified compiler defect must first be fixed. FAILS means a false/unfair claim or an unsatisfied standing owner gate. Fixed r5 misstatements are recorded below, not silently counted as outstanding r6 failures. The first three may be posted as bounded design directions, never as implemented support. ZIP must not be ratified before its strict paired win; Main explicitly reaffirmed that rule.

## Reproduction boundary

Independent probes used gate-p289's published compiler and matching toolchain root, NO_COLOR=1, unset LD_LIBRARY_PATH and nice -n19. No compiler builds or broad suites ran. All top-level once-r5 and fnval-r5 `.jet` programs ran; so did once scratch v1/v4/v5/v6/t1/t2, original fnval-fc1/fc8/fc9, once-on3b/on6 and service exclusive-tree. Full combined diagnostics are retained in artifact://97765. Source models and proposed syntax are distinguished throughout; no foreign-language execution is claimed.

For Service I copied only the gate's Actor.jet and Service.jet into `../probes/service-r5/review-r6/tc`, symlinked other gate modules, and applied the supplied helpers with the edit tool. I ran only `jet test <overlay>/Core/Service/Service.jet --filter=PROBE5 --capture=all`. The supplied run.sh was not executed: it makes a different overlay with a fixed p288 root. The four tests passed on p289.

For ZIP I built the nine supplied small programs with `jet build -O2 --quiet --output-dir=<scratch> <program>`. Six performance binaries ran in three sequential round-robin rounds through the supplied perfcount.py; ownership and both stage-timing binaries also ran. These are program builds, not a compiler build or suite. All performance executions printed the same sum; the supplied counter runner discards child exit status, a limitation that must be fixed for a ratification receipt.

## Claims found and repaired

### ONCE

1. **“Today's move checker enforces every single-call rule.”** Too broad. owner-pass and owner-branch produce 8 but each closes its resource twice. places-field-return-task produces 9 and closes twice. places-task-local rejects its named local at task handoff with E0121. r6 reports those failures, rather than reducing successful evidence to the numeric result. These map to #3645 c14-c16.
2. **Model equals public OneShot.** False framing. It is a generic `OneShot<E>` record exposing E, an ordinary fn field and a `.call` method. It does not establish erased callable construction, conversion or generic signatures. Also, today's uncopyable fn field accidentally makes the model uncopyable; FNVAL's proposed copyable fn law would remove that reason. r6 requires unconditional OneShot noncopyability even with empty or copyable captures.
3. **Retained resource owners cannot use the same owned helper; use Shared.** An avoidable loss presented as inevitable. An ordinary borrowed helper is not the requested same-helper adapter; Shared changes ownership and is not universally available or appropriate. Rust's `FnOnce for &F` / `&mut F` is a stronger existing alternative. r6 specifies a temporary owned OneShot borrowing a repeatable fn/Unique invocation capability, preserving existing loan lifetime/exclusivity through forwarding and forbidding genuine-OneShot resets. Consuming the temporary does not consume the retained owner. This is a proposed conversion, not a passing Jet receipt.
4. **Loan rejection proves no scoped adaptation can exist.** Invalid inference. Existing store/task probes reject today's borrowed fn with E0201/E0120; they do not test a lifetime-checked OneShot adapter. r6 removes the impossibility claim.
5. **“Error cleanup” proved.** zero-call-error returns an optional None, not a failable error, panic or cancellation. r6 narrows the receipt and keeps those exits as LIMIT.
6. **SingleUse must be invoked in that helper.** Too narrow: forwarding/returning ownership can discharge its local obligation. r6 preserves the actual duty rather than adding an exactly-once invocation rule.
7. **B is inferior because it needs &&.** Still not a fair distinction: consuming A needs ownership too. B's real restriction is synchronous parameter-only scope. C remains an executable no-copy solution: once-on3b and on6 each print 8.

Observed negatives: owner-second-helper, owner-alias, owner-forward-twice, owner-loop and places-field-twice give E0121; owner-borrowed E0201; uncalled SingleUse E0140. Invoked SingleUse prints 8. Ordinary zero-call prints `not called`, then one close. Retained ordinary borrowed helper prints 8, 9, 2 and one close. Scratch v1/v4/v6 reproduce duplicate cleanup; v5 correctly reports E0209 while generic v1 accepts its missing move; t1/t2 reject named-local transfer.

I agree with A as a bounded direction after scoped adaptation is specified. I do not claim the record model proves its implementation. No fourth option was added: the stronger scoped-loan alternative is incorporated into A rather than creating a second once mechanism.

### FNVAL

1. **“A task's copy cannot race its creator on hidden state.”** False as an unconditional consequence of copying. Shared, Cell and nested callbacks retain reachable edges; crossing and cycles are separate facts. r6 removes the guarantee and requires the complete ratified proofs, including the actual #6048 regression. fc8's E0121 only proves that particular current program is rejected, not that every creator/task case is safe.
2. **Models demonstrate closure copying.** copy-cleanup/copy-crossing copy ordinary Env records, not callable values. unique-callback is a generic Unique<E> record, not erased Unique<fn>. r6 states the boundary.
3. **Unique-copy refusal proves the future nominal capability.** It does not: the model's ordinary fn field is itself noncopyable today, besides its unique resource. r6 requires Unique noncopyability even with copyable captures after FNVAL changes plain fn.
4. **Exact task print order.** This run printed 201, 202; the order of the two guarded mutations is not guaranteed by joining their handles in source order. r6 calls the result order unspecified.
5. **Unique/OneShot inherently satisfy I8 because they form a ladder.** A slogan is insufficient. r6 restricts them to capabilities in one callable family, existing ownership/copy/loan/crossing facts, no nested competing wrappers and no runtime copy refusal. It preserves input modes/effects/results and requires separate-compilation proof. They are not a second generic Unique<T> ownership container.
6. **Independent snapshot or free copies.** Ordinary list/map copies may allocate on writes; Shared retains owners and can contend. r6 keeps capture-dependent COW, retention and cleanup costs, with short guards, independent ordinary state and promptly released copies as mitigations.
7. **B is only a dangerous aliasing option.** B's best case is stable action identity and resource retention without copying the resource; Swift/Kotlin/OCaml show this is a serious model. It still needs synchronization/reentrancy/cycle design and an explicit fn-copy-law amendment. r6 states that fairly.

Observed record model: ordinary and map mutations diverge; Shared contents count 1, 2, 3. Error path leaves first's own length 2 and second's 3. Shared resource owners fall 2 to 1 when the second copy drops; it closes once. Unique model prints 2, 3, 4, 5 and closes once; its copy reports E0211. Actual fnval-fc1 and Button fc9 still report E0211. No actual copied-closure topology oracle was run or fabricated.

I agree with A as a bounded capability direction, not as a proven race fix or a universal deep-copy guarantee. C remains strong: two fresh closures around explicit Shared state are executable without copying a function value. The actual topology and erased public type conversion remain LIMIT, not failed design assertions once stated honestly. No new ballot option was added.

### SERVICE

1. **Stopped-inbox loss is repaired by the prototype.** Holds for the sequential helpers: [unsent] returns and room counts remain 4. Answered port 1 and refused port 2 each close once. This is stronger than r3's discarded unsent envelope, but not yet public Refused<Q> proof.
2. **“Non-waiting submit” means atomic/cancellation-free admission.** Unsupported. Actor.submit is declared Time.Wait and calls Sender.send on the unbounded actor channel; its request constructor is arbitrary callable code. It avoids waiting for a reply, which is weaker. r6 removes the blanket assertion.
3. **exclusive-tree proves no in-process admission race.** Overclaim. The probe uses today's service.call, not admit5, and does not exercise staged resolve/reserve/publish, alternate handles or shared rollout authority. E1101 is useful crossing evidence, not a linearizability proof.
4. **Waiting is outside the exclusive tree borrow.** call5 retains its &ServiceTree parameter through await5. Splitting admission into another function does not end that outer borrow. r6 requires narrow admission coordination and no registry coordination across reply waits, including full-mailbox and handler-reentry progress.
5. **One resolver is fully migrated.** Only private resolve5 plus selected explicitly rewritten maintenance paths are demonstrated. r6 states canonical public migration as a LIMIT, with distinct delivery/maintenance liveness policies rather than an I8-violating second identity mechanism.
6. **Volatile queue commit proves durable ownership/retry.** It does not. r6 distinguishes volatile publication from durable journal commit, keeps post-admission Ambiguous honest, and names coordination/journaling/recovery costs. External effects do not become exactly-once through a lost reply or retry key alone.
7. **B/C necessarily lose payloads.** They do not; recovery is independent of addressing policy. Existing r5's fair treatment is retained. Typed-witness C must match authority, tree and worker, not just compatible Q/R. No extra option was needed.

Observed PROBE5 output: admitted-before-handoff 10; admitted-before-drain 20; after-drain returned=true. Partitioned [a,b] returns; reconciled call answers 2; foreign delivery and maintenance reject authority. Old-id drain is Stale; resolution permits drain and later returns [late]. These sequential observations do not include paused interleavings, restart, cancellation or durable commits. The new public Core generic error still has a tracked lowering prerequisite, #6048 c9; this review did not rerun a standalone reproduction of that backend defect, and does not claim a fresh backend-failure receipt for it.

I agree with A's bounded identity/admission contract. The remaining durable protocol and lifecycle receipts are LIMIT; public generic lowering is PREREQ. They are first-build requirements, not reasons to mislabel existing support as complete.

### ZIPCOLUMNS

1. **Voting A can replace strict win with parity.** Violates the standing owner gate. AGENTS.md:251-254 explicitly requires a paired two-program cell and a strict win before ratification. Main reaffirmed this, with #6177 c17 tracking closure. r6 removes the attempted waiver. This remains FAILS, not merely LIMIT.
2. **No implementation can beat an identical record.** A tautology does not prove candidate code is identical, especially with validation and recovery absent from the hot baseline. Compare complete equivalent semantics and apply general lowering fixes to both sides. No Columns candidate was measured.
3. **B lacks checked lengths or recovery.** False and unfair: the supplied plain-record ownership model already returns unique inputs on mismatch. B's real cost is handwritten row-type plumbing, not unavoidable data loss or copying. r6 repairs both expert and owner-facing text.
4. **Fastest in all measured forms.** The author's own narrow literal margin was noisy; this independent loaded run does not reproduce the universal ranking. r6 records measured values without claiming a winner or substituting instruction count for required wall/RSS evidence.
5. **Cycles cannot be inflated by load.** False. The runner's own docstring distinguishes cycles/wall from instructions. It also lacks enabled/running normalization for multiplexed counters and discards child exit status. r6 qualifies the receipts. They are not a robust ratification runner.
6. **Every record-returning function pays the measured penalty.** Unsupported extrapolation. The 2.880 G instruction delta is observed for this workload; the lowering cause is an inference, tracked as #6923 c11. Any structural fix must improve ordinary helper code too, not privilege Columns.
7. **Padding rules out a 4K alias explanation.** The pad run changes requested bytes by 2112, but no buffer-address receipt establishes the claimed page offsets. It is not a demonstrated isolation experiment. r6 does not repeat that causal claim.
8. **Ownership model proves generic Columns/lifetimes/failed growth.** It proves a useful plain-record witness only. Its one reported join allocation includes construction of the v list, so it does not demonstrate descriptor-transfer allocation. Alias stays 1.0 while table becomes 9.0; write adds one detachment allocation; split yields lengths 3/3; mismatch yields 2/1 and closes x1, x2, v1 once. Zero-field/count, actual field windows and transactional failure remain LIMIT.

I agree with A's semantic direction only within the stated limits, not its ratification now. B is strong and must be the optimized checked comparator. No option was added; the strongest plain alternative was restored inside B.

## Independent ZIP measurements

All six program outputs: sum 10391999.750000002. Wall seconds include process startup and scheduling. Counters are unnormalized user-mode raw measurements from the supplied runner; do not infer a universal performance ordering from this small loaded sample.

| plain program | wall seconds, three rounds | peak RSS KiB, three rounds | allocations | requested bytes | approximate instructions |
| --- | --- | --- | --- | --- | --- |
| adopt_move | 2.338083, 3.524125, 3.734922 | 254116, 254088, 254120 | 12 | 256003288 | 12.964 G |
| literal | 3.660903, 3.307519, 3.469436 | 381092, 381096, 381096 | 13 | 384003288 | 12.964 G |
| adopt helper | 3.797315, 3.781734, 3.386704 | 254116, 254088, 254116 | 12 | 256003288 | 15.844 G |
| rows | 4.353329, 2.607405, 1.700441 | 505992, 506020, 506024 | 16 | 512003380 | 15.556 G |
| copy | 2.771625, 4.359244, 2.386634 | 508068, 508072, 508072 | 20 | 512003472 | 13.892 G |
| adopt_move_pad | 5.799242, 2.677941, 1.319801 | 254120, 254116, 254088 | 12 | 256005400 | 12.964 G |

Stage timing probes also reproduce their outputs, but instrumented work changes allocation counts: t_adopt reports build/update 646/1024 ms, t_literal 442/1300 ms. Neither is a Columns candidate or an isolated proof of the alleged backend cause.

## Closure disposition for every review-r3 row


Each original closure row appears below. Classification applies to the remaining r6 item after repairing false r5 wording. A LIMIT is honest scope for a design vote, not a passing implementation test. Each corresponding build card's **first criterion** must prove all its LIMIT rows and close its PREREQ rows before support is claimed.

### SERVICE-CURRENT1

| open item from review-r3 | classification | evidence and remaining limit / cost | mitigation and probe proving closure |
| --- | --- | --- | --- |
| Payload-preserving admission | LIMIT | Sequential stopped inbox and unique messages now recover correctly; public Refused<Q> is unbuilt. Returned ownership adds error-path plumbing. | Port the same stopped-inbox/unique-owner checks to public call; verify identical payload, non-Ambiguous reason, one cleanup and no permit leak. |
| Coordinated resolve/reserve/publish | LIMIT | E1101 and sequential handoff do not prove all interleavings. Coordination costs work/serialization; call5 currently retains its tree borrow through wait. | `service-admission-interleavings`: pause after resolve and reserve; race handoff, rollback, drain, partition and revocation, including other processes; full mailbox and reentrant handler must progress. Require one permitted admission or returned Q, exact permit cleanup and no coordination across reply wait. |
| One identity resolver, distinct maintenance policy | LIMIT | Selected private resolve5 paths work; public mailbox-depth, drain and failure maintenance are not all migrated. Shared resolution costs per-call checks. | `service-partition-maintenance`: partition, refuse delivery with payload, reconcile via retained identity, then deliver; also mailbox depth, drain and failure APIs; foreign authority always fails. One canonical resolver, operation-specific liveness policy. |
| Durable admission/idempotency | LIMIT | Only volatile publication is prototyped. Journaling, receipt retention and duplicate suppression cost I/O/state; external effects may remain uncertain. | `service-durable-commit`: stop just before/after journal commit, handoff/restart and retry; verify receipt/key scope, precommit recovery, postcommit ownership and documented duplicate suppression. Never assert exactly-once external effects without atomic effect protocol. |
| Cancellation/deadline ownership | LIMIT | No cancellation boundary receipts. Reserved unpublished messages need exact ownership and permit cleanup. | `service-cancel-admission`: cancel before reserve, reserved/unpublished, and published; count unique cleanup, return/cleanup before commit and ambiguity only after admission; no leaks or duplicate owners. |
| Restart/discovery/public migration: lifecycle part | LIMIT | Dead transport replacement and discovery revocation/expiry remain unimplemented receipts; stale capability/security checks cost resolution work. | `service-restart-discovery-error`: replace actor/process; exercise old capabilities, revoked/expired signed discovery and pinning; no dead actor reuse or authority widening. |
| Restart/discovery/public migration: Core generic error | PREREQ | #6048 c9 tracks generic Core Refused<Q> lowering; model uses existing SendError<Q?>. | Fix lowering, then compile exported `R Refused<Q>!` with move-only Q and every dispatcher/error-matching caller; remove old contract in one cutover. |

### FNVAL-COPY1

| open item from review-r3 | classification | evidence and remaining limit / cost | mitigation and probe proving closure |
| --- | --- | --- | --- |
| Static erased copy capability | LIMIT | Plain fn / Unique / OneShot now have a proposed type-level capability contract, but record models expose E and prove no erased conversion. Migration and inferred conversion add type complexity. | `fnval-erased-copy`: separately compiled makers, fields, Button, collections and forwarding; eligible copies compile, unique owners repeatedly call/move, invalid copy fails at public boundary. Include empty Unique and OneShot under the new copyable fn law. Preserve all input modes/effects/results. |
| Actual generated capture copy / topology oracle | LIMIT | No real callback copy or updated topology oracle exists. Captures may retain strong owners and delay cleanup. | Run actual shared_owner_topology copy path at O0/O2; derive every oracle value from output and owner transitions, including failed append. Review against D-SHARED-CYCLE2, not a guessed count. |
| Complete hidden crossing/cycle summaries | LIMIT | Explicit-record task copies work; fc8 rejects E0121, but neither closes full #6048 creator/task coverage. Shared/Cell/nested edges remain. | `fnval-copy-crossing-cycles`: actual copied callbacks with Shared, Cell, nested callbacks, unique owners and callback fields; creator/task aliases and cycles; use single crossing/ownership proof and reject unsafe sharing. No fresh-environment exemption. |
| Copy cost, cleanup and failure | LIMIT | Record list/map independence and shared cleanup are observed, not actual callable copying. COW, allocation, retained-owner and reentrancy costs remain. | `fnval-copy-cleanup`: real callback copies, nested mutations/error, both drop orders, reentrant call; report allocations and owner counts. Keep guards short, release copies promptly, choose ordinary independent captures where possible. |

### CALLBACK-ONCE1

| open item from review-r3 | classification | evidence and remaining limit / cost | mitigation and probe proving closure |
| --- | --- | --- | --- |
| Owned invocation, no helper reset | LIMIT | Record negatives correctly reject aliases/double forwarding/loops; real OneShot conversion and permission erasure remain unbuilt. | `oneshot-owner-transfer`: real named OneShot through owned helper; reject second helper, alias, double-forward and consuming invocation through ordinary read loan; branch-exclusive calls pass; repeat loop fails. Empty/copyable environments remain noncopyable. |
| Retained repeatable adaptation | LIMIT | r6 removes the avoidable loss by specifying temporary owned adapters using existing callable loans. No Jet receipt implements it; lifetime/exclusivity restrictions are real costs. | `oneshot-repeatable-loan`: same helper twice with one retained resource-owning repeatable callback, then direct call, without copying resource. Reject escape, overlap and true-OneShot reset. Move ownership for longer storage; do not force Shared. |
| Fields/returns/generic/task coverage | LIMIT | Explicit inline-task record prints 9 but not the erased public contract; mode/effect propagation and partial-field cleanup remain unproved. | `oneshot-places`: actual OneShot through field, return, generic forwarding and named-local task, preserving input modes/effects/results; reject reuse and invalid partial cleanup. |
| Fields/returns/generic/task compiler lowering | PREREQ | #3645 c14-c16: duplicate closes, missing generic move marker check and named-local task rejection reproduced on p289. | owner-pass/branch and field/task must close once; scratch v1 must require the same move as v5; places-task-local plus t1/t2 must transfer once without false E0121. Run O0/O1/O2. |
| Zero/error/panic/cancel/SingleUse | LIMIT | Ordinary zero-call and optional early return close once; uncalled SingleUse rejects. Failable error, panic and cancellation remain absent. | `oneshot-zero-cleanup`: counted unique captures and SingleUse on every exit, exactly one cleanup or correct duty rejection. Forward/return may discharge local ownership without invocation. |
| One fact across positions and crossings | LIMIT | Models prove no unification of local, Cell, Shared and task semantics. Lifetime/sendability/cycles are independent costs. | `oneshot-unified-crossing`: same invocation fact in all positions, creator/task aliases and captured loans; reject unsafe crossing, no allowlist, runtime reset or second once wrapper. |

### ARRAY-ZIPCOLUMNS1

| open item from review-r3 | classification | evidence and remaining limit / cost | mitigation and probe proving closure |
| --- | --- | --- | --- |
| Built candidate strict performance gate before ratification | FAILS | No actual Columns candidate; parity amendment was unauthorized. Main reaffirmed AGENTS.md:251-254 and #6177 c17. | `zipcolumns-paired-candidate`: before ratification, actual candidate strictly beats strongest equivalent checked plain form in paired wall/RSS cell, with all raw runs, correct exits and memory/allocation evidence. Apply structural fixes equally. A future build promise or parity does not close this owner gate. |
| General helper-return lowering | PREREQ | #6923 c11 owns the suspected general penalty; this workload has an observed 2.880 G instruction difference, not a proved universal cause. | General backend A/B: eliminate avoidable return-slot work in plain helper and second structurally different record-return program; rerun both sides of paired candidate cell. No Columns-specific benchmark bypass. |
| Owning transfer / aliases / split / unique mismatch / count | LIMIT | Plain model proves useful alias, split and unique recovery cases, not generic Columns, zero-field or name-collision behavior. Aliases may detach and allocate. | `zipcolumns-ownership`: actual Columns with aliases, mutation and split; exact COW cost; mismatch returns all unique inputs once; zero-field explicit and invalid counts, field named count; move unaliased inputs when possible. |
| Borrowed windows / growth failure | LIMIT | No real view/window conversion or multi-column failure receipt. Separate field buffers and transactional reservation cost remain. | `zipcolumns-loans-growth`: reject view escape/growth/unique move-out; exercise field-window arguments; inject each reservation/commit failure after prior growth, preserving values/lengths and exact cleanup. Reserve capacities; retain the one ratified layout. |

## Sources and rule checks

- Rust FnOnce: https://doc.rust-lang.org/std/ops/trait.FnOnce.html — consuming self; borrowed Fn/FnMut implementations support retained repeatable owners; Send is not implied. Closure traits: https://doc.rust-lang.org/reference/types/closure.html#other-traits — Clone/Copy depend on capture modes and captured types; Send/Sync remain separate.
- C++ target copying: https://en.cppreference.com/w/cpp/utility/functional/function/function — copies target and requires a copy-constructible stored target; references/shared ownership do not become deep snapshots. move_only_function invocation: https://en.cppreference.com/w/cpp/utility/functional/move_only_function/operator() — move-only storage is not an affine one-call guarantee.
- Swift: https://raw.githubusercontent.com/swiftlang/swift-book/main/TSPL.docc/LanguageGuide/Closures.md — reference aliasing is explicit; @escaping is a separate lifetime promise, not call count. Kotlin closures: https://kotlinlang.org/docs/lambdas.html#closures — may mutate captured outer variables. Kotlin callsInPlace: https://kotlinlang.org/api/core/kotlin-stdlib/kotlin.contracts/-contract-builder/calls-in-place.html — invocation during owner call, optionally bounded count. OCaml: https://ocaml.org/docs/mutability-imperative-control-flow — create_counter hides a mutable ref; separate factory calls produce independent counters. These make the shared-identity and explicit-state alternatives serious, not inferior strawmen.
- Service comparisons: Erlang gen_server accepts pid or registered server identity, https://www.erlang.org/doc/apps/stdlib/gen_server.html#call/2 ; Orleans references encode logical identity independent of physical activation, https://learn.microsoft.com/en-us/dotnet/orleans/grains/grain-references ; Tokio unsent-value error, https://docs.rs/tokio/latest/src/tokio/sync/mpsc/error.rs.html . None promises unconditional availability or safe repetition of external effects.
- Odin borrowed SOA grouping: https://odin-lang.org/docs/overview/#soa_zip-and-soa_unzip . It does not demonstrate Jet owning recovery or transactional growth.
- Marker grammar rechecked: Compiler/JetFoundation/Data/Syntax/Markers.jet:4-6 owns no duplicate marker-name list; RULE_PREFIX at line 19 is #. Compiler/Prelude/Markers.jet:1-20 is the canonical name/site registry. Compiler/JetParser/Source/Parser/Items.jet:876-886 already parses a parameter marker and registers unknown names for E0927. B needs registration and semantic fact propagation, not a second marker grammar. Unique and OneShot add callable type contracts, not markers.
- Ratified payloads rechecked for D-COPY-WORD1=A, D-FNVAL-WRITE1=A, D-SHARED-CYCLE2=A, D-CONC-CROSS1=A, D-SOA-SITE1=A, D-ARRAY-COLUMNS1=A and D-MOVE-SIGIL1=A. Preserve later &&/.move spelling rather than replaying historical ^ spellings. I1 forbids blessing duplicate cleanup or races; I8 requires one callable/ownership/crossing mechanism, one resolver with policy-specific checks, and one Columns representation.

## Posting boundary

The first three verdicts authorize only the bounded ballot direction, not implementing past an unmet compiler prerequisite, claiming a passing real feature receipt, or shipping unproved safety. No output should advertise closure copying, temporary OneShot adaptation, durable service delivery or Columns joins as working today. ZIP's FAILS gate must not be recast as LIMIT. r5 historical timing claims are not candidate wins. Schema validation alone does not establish any of these claims.

## Final validation receipt

All four r6 payloads pass `Tools/Agent/Ballots/Validate.jet` on gate-p289. The final combined run passed FNVAL, SERVICE and ZIP; ONCE initially exceeded the 90-word paragraph cap. I shortened that paragraph without changing the contract and its targeted validator rerun passed. Schema PASS does not change ZIP's substantive fails verdict.

The combined run also independently reran `../probes/fnval-r3/review-c.jet` on gate-p289: it printed 101, 202, 303. This closes the fresh receipt for C's explicit shared-state witness, not for actual callable copying.

## Dispositions

<!-- audit-dispositions:v1 -->
| finding | disposition | target or reason |
| --- | --- | --- |
| D-SERVICE-CURRENT1 holds within stated limits | card | #6048 holds the reposted r6 ballot; its build work first proves every LIMIT row in the SERVICE-CURRENT1 table above |
| D-FNVAL-COPY1 holds within stated limits | card | #6548 holds the reposted r6 ballot; its build work first proves every LIMIT row in the FNVAL-COPY1 table above |
| D-CALLBACK-ONCE1 holds within stated limits | card | #6780 holds the reposted r6 ballot; its build work first proves every LIMIT row in the CALLBACK-ONCE1 table above |
| D-ARRAY-ZIPCOLUMNS1 fails the strict pre-ratification gate | card | #6177 c17: a built Columns candidate must strictly beat the plain form in the paired cell before the ballot is posted |
| PREREQ generic Core success and error types in failable results | card | #6048 c9 |
| PREREQ function-value double close, generic move marker and task local transfer | card | #3645 c14, c15 and c16 |
| PREREQ returned record built in place | card | #6923 c11 |
<!-- /audit-dispositions -->
