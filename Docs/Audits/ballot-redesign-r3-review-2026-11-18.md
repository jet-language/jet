# Adversarial review of ballot redesigns — 2026-11-18

Author family: Claude Opus. Reviewer family: GPT. Fresh reviewer: BallotReview2 (runtime agent BallotReview2-2). The owner requires removed downsides, not renamed costs or deferred evidence. All r4 payloads are review artifacts only; no Tower or git writes.

| finding | disposition | target or reason |
| --- | --- | --- |
| SERVICE-ADMISSION: route selection and admission are not coordinated by the prototype | decision | D-SERVICE-CURRENT1.r4.json, not ready; coordinated-admission prototype required |
| SERVICE-UNSENT: stopped actor loses a definitely unsent request | decision | D-SERVICE-CURRENT1.r4.json, not ready; independent gate-p287 counterexample |
| SERVICE-MAINTENANCE: delivery-only checks prevent reconciliation | decision | D-SERVICE-CURRENT1.r4.json requires one identity resolver with operation-specific checks |
| SERVICE-FAIRNESS: B and C can also return unsent values | no-action | Repaired in r4; payload recovery is independent of address policy |
| SERVICE-IDEMPOTENCY: keys do not guarantee exactly-once external effects | no-action | Removed the unconditional claim in r4 |
| FNVAL-COPYABILITY: universal repeatable copying needlessly forbids unique owners | decision | D-FNVAL-COPY1.r4.json conditionally copies; erased capability contract still blocks posting |
| FNVAL-SHARED: snapshot, contention-free and ownership-edge exemptions are false | no-action | Corrected in r4; ordinary Shared capture costs remain explicit |
| FNVAL-ORACLE: explicit-record model does not prove closure ownership counts | decision | D-FNVAL-COPY1.r4.json requires actual topology receipts before changing the oracle |
| ONCE-OWNERSHIP: a read loan cannot consume its caller's one-shot owner | decision | D-CALLBACK-ONCE1.r4.json requires && and a proved retained-caller conversion |
| ONCE-EVIDENCE: local single-call probes do not establish fields, returns or tasks | decision | D-CALLBACK-ONCE1.r4.json remains not ready pending actual OneShot receipts |
| ONCE-GRAMMAR: parameter #Name syntax already exists; Once is unregistered | no-action | Corrected r4; register names in Compiler/Prelude/Markers.jet, not Syntax/Markers.jet |
| ZIP-PERFORMANCE: no built Columns measurement and adoption is not fastest | decision | D-ARRAY-ZIPCOLUMNS1.r4.json remains not ready; pre-ratification gate is unmet |
| ZIP-FAIRNESS: ordinary consuming records already avoid element copies | no-action | Corrected B and the same-program comparison in r4 |
| ZIP-ALIASES: && does not establish unique buffers or remove later COW | no-action | Removed explicit-copy-only assertion; earlier aliases and detachment costs remain |

## D-SERVICE-CURRENT1 — not ready

I agree with stable logical identity as a direction. I do not agree that the supplied prototype removes every listed loss. The revised payload states the missing contract precisely and remains unapproved; do not post it as a passing design.

### Independently rerun evidence

Compiler: `$HOME/.cache/jet-dev/lander3-tmp/gate-p287/tree/Compiler/JetCLI/.jet/build/jet`. Set PATH, NO_COLOR=1 and JET_TOOLCHAIN_ROOT explicitly; unset LD_LIBRARY_PATH. Scratch is `../probes/service-r3/review-r4/`. Its toolchain overlay copies the gate's Service.jet, not a guessed git revision; other modules point to the gate. Appended helpers reproduce the author's refuse/deliver_gate/send_now/call_now implementation without adding a lock. Only the two REVIEWR3 probes and one REVIEWR4 probe ran, not a suite.

Sequential rerun: before 10; handoff 2; today's old call Stale; call_now 20; rollback 1; call_now 30. Early refusal returns [early]. Partitioned pinned shard returns [a, b], another worker answers 105, reconciliation then answers 2. Foreign send returns [x]. Handler failure returns no message. Old drain address is Stale; draining the current id then calling returns [late]. Both original shape probes pass.

The new REVIEWR4 probe closes `api.inbox` after starting the tree, then calls call_now with [unsent]. Output: `closed-before-enqueue:message gone`. This is not an uncertain admitted request: the inbox was closed before enqueue. The negative finding probe passes by observing the defect.

The standalone generic-error-user probe prints `no [a]` and `send [b]`. This proves the user-declared error shape only, not a public Refused<Q> or its Core lowering.

### Claimed removed losses

- **Retained address refresh:** removed for the sequential helper's local handoff and rollback. Restart, actor replacement, trust expiry and revoked authority remain distinct. The same private actor cannot simply be reused across process restart.
- **Every refusal returns its message:** false. `Core/Actor/Actor.jet:34-40` constructs a request, then returns None when inbox.send fails, discarding its SendError envelope. call_now maps that None to Ambiguous. The closed-inbox counterexample demonstrates a definitely unsent lost payload. r4 requires separate payload-preserving admission and reply waiting; this is not yet proved.
- **Refresh/send race:** the public refresh step is gone, but the supplied deliver_gate returns before room.try_acquire and inbox.send/ask. It holds no registry guard across them. Claiming an existing atomic gate is false. A guard held across blocking send or reply wait would instead introduce contention or deadlock. r4 specifies coordinated reservations visible to drain, publication, cancellation and lock release before waiting. No concurrent proof of that protocol exists here.
- **No new name:** no new refresh method, but Refused<Q>, its optional payload and the R ServiceError! amendment are public learning and migration costs. Existing callers matching ServiceError change. r4 names the greenfield cutover rather than promising unchanged callers.
- **One gate for everything:** literal reuse of delivery checks rejects partitioned workers before reconcile_worker can repair them. r4 separates one identity resolver from operation-specific authorization/admission checks, without adding an independent routing mechanism.
- **Idempotency removes the remaining uncertainty:** an accepted key can suppress duplicate admission within a retained scope. It cannot generally make arbitrary external side effects exactly-once. Removed the unconditional retry promise. A dead caller also cannot receive ownership on cancellation; cleanup and ownership destinations need defined semantics.
- **B/C inherently lose messages:** false comparison. Both can use the same returned-message error. r4 gives that recovery to every option and compares their actual refresh, alias and directory-availability costs.

### Sources and rules

Read current dev `Core/Actor/Actor.jet:20-45`, service function declarations and `feed_current` directly. The gate overlay is the executable source for reruns. Read Tokio's source: https://docs.rs/tokio/latest/src/tokio/sync/mpsc/error.rs.html — SendError<T>(pub T), and Full(T)/Closed(T) both retain the value. Erlang gen_server documentation and Orleans references were already checked in the preceding independent review; r4 removes the false implication that Erlang disallows pid calls.

D-SERVICE-IDENTITY1 freshness and trust remain mandatory; directory generations are distinct from local route generations. D-SERVICE-UPGRADE1 pinning must remain. Returning Refused<Q> explicitly amends D-SERVICE-REPLY1's error signature. I1 requires preserved ownership, safe cancellation and no authority widening. I8 means one resolver, not one unsuitable liveness policy for every operation. No marker grammar is proposed.

## D-FNVAL-COPY1 — not ready

I agree with ordinary field-wise copying, not universal copyability or forced sharing. I strengthened A to conditional copying and retained the other two options. The unresolved erased-function capability contract prevents posting; a hypothetical generated copy operation is not proof of that contract.

### Independently rerun evidence

All probes use gate-p287 with its own toolchain root. `../probes/fnval-r3/env-model.jet` prints 104, 104, 204, 4, 1; it copies a struct with a Bucket and Shared owner. `shared-capture.jet` prints 101, 202, 303. `stored-lambda-owns.jet` emits E0204. The original `new/probes/fnval-fc1.jet` and fnval-fc9.jet independently emit E0211 for a returned callback and Button. `../probes/fnval-r3/review-c.jet` compiles the full option C maker and prints 101, 202, 303. Thus the real current alternative is executable, not a strawman.

### Claimed removed losses

- **Independent snapshot / no contention:** ordinary copied list state is independent on mutation, but a copied Shared remains shared. It retains a strong owner and can contend on guards. Cell remains subject to its own access and crossing rules. Removed all deep-snapshot and contention-free claims. Short guards, promptly released copies and distinct state are concrete mitigations; copying alone cannot supply independent shared-resource state.
- **No hidden-capture proofs:** false even if repeatable functions are all copyable. Lifetime, cleanup, ownership graph and task crossing still depend on captures. `D-CONC-CROSS1=A` specifically requires complete facts at every crossing, not exemption for fresh environments.
- **No ownership edges from a copy:** copying a Shared capture adds a strong owner, exactly as the model's counts show. Graph summaries must include it. `D-SHARED-CYCLE2=A` allows no second collector or unchecked closure special case.
- **Unique resources must go in Shared:** an avoidable loss, not a theorem. A unique resource need not exist in two callbacks: keep its repeatable owner move-only and refuse only copy(). Rust's conditional Clone is the missing strong design and is now A. Shared changes aliasing, cleanup and synchronization; inserting a wrapper is not a semantics-preserving universal fix.
- **Every value gets the same rule:** COPY-WORD1 applies to every copyable value. It does not require every repeatable callable to be copyable. Conditional copying needs static capability information preserved through public erased fn signatures, returns, fields and collections. The draft supplied no such contract. r4 identifies that exact blocker rather than creating runtime copy failure or a new copy spelling.
- **Fixed one count per capture:** false for scalars, nested records, Shared and later COW detachment. Replaced with ordinary captured-value costs.
- **Oracle already proved:** env-model is not a closure-copy implementation. Read fixture lines 22-29 and 70-77 and oracle lines 10-14 directly. Changing 102/202/302/2/1 to predicted values requires actual ownership/cleanup receipts, not an arithmetic offset of a struct experiment.
- **B contradicts existing owned capture semantics:** owning a record does not preclude future aliases to it. B's honest costs are copy-law amendment, retained captures, synchronized shared mutation, reentrancy and graph/crossing proofs. It preserves action identity and need not duplicate a unique resource. Reworded fairly.
- **Old creator/task race receipt establishes today's behavior:** removed the unrerun old-gate assertion. The regression tracked on #6048 must run against the implemented copying/crossing design; A is not a race fix by itself.

### Sources, rules and remaining cost

Read Rust Reference's closure Other traits section: Clone/Copy are conditional on capture mode and captured types; Send/Sync are separate. https://doc.rust-lang.org/reference/types/closure.html#other-traits . Read FnOnce documentation: invocation consumes self, while &F and &mut F implementations require Fn/FnMut respectively. https://doc.rust-lang.org/std/ops/trait.FnOnce.html .

C++ std::function copies its target rather than making reachable state deeply independent; reference and shared-pointer captures remain aliases. Swift closure assignment aliases captured state; escaping is a separate lifetime question. These external examples were source-checked, not run with foreign compilers. No claim of a foreign-language executed demo is made.

I1 keeps unique cleanup, SingleUse, complete cycle proofs and creator/task synchronization. I8 keeps one ordinary copy spelling and one ownership/crossing mechanism. No marker or grammar addition is proposed. A conditional callback owning a genuinely unique resource cannot itself copy: move it, acquire a distinct resource, or opt into valid shared ownership. Shared retention and contention remain real and mitigated, not erased.

### Completed payload validation

Gate-p287 Validate.jet reports `ok D-SERVICE-CURRENT1.r4.json (D-SERVICE-CURRENT1)` and `ok D-FNVAL-COPY1.r4.json (D-FNVAL-COPY1)`. The validator requires draft:false even for review artifacts; that schema flag is not approval to post. Both adversarial verdicts explicitly say Not ready.

## D-CALLBACK-ONCE1 — not ready

I agree that one consuming callable contract should cover helpers, stored fields, returns and tasks. I do not agree that the supplied borrowed-parameter design preserves ownership or establishes unchanged callers. A now takes &&OneShot<F>; retained-repeatable adaptation is explicitly unresolved, not silently dropped from scope.

### Evidence and repairs

Independent gate-p287 runs: local-once prints 8; local-twice reports E0121 after its first call; passed reports E0121 at Log's move. caller-keeps prints 8 then 2 through an ordinary borrowed fn parameter. task-once prints 8. None implements OneShot or its conversions. The existing explicit-state `new/probes/once-on3b.jet` and optional-branch `once-on6.jet` each print 8; C is a real zero-copy alternative.

The new `../probes/once-r3/review-marker.jet` reports E0927, UnknownAppliedRule for #Once, not a tokenizer failure. Read `Compiler/JetFoundation/Data/Syntax/Markers.jet:1-19`: it owns # syntax and explicitly assigns marker names to Compiler/Prelude/Markers.jet. Read `Compiler/JetParser/Source/Parser/Items.jet:876-886`: parameter markers already parse. Therefore B needs registration and preservation of the invocation fact through parser/AST/signatures/tooling, not a second marker-name list or a new # grammar.

The original A consumed a read-borrowed parameter while promising the caller kept its callback. A read loan cannot silently transfer a unique captured owner; passing the same named one-shot into two helpers must not reset its allowance. r4 uses explicit ownership. A retained repeatable function can potentially use a scoped callable loan, like Rust's &F/&mut F FnOnce implementations; that adapter must not become an escaping borrowed OneShot or duplicate a one-shot allowance. No such Jet conversion is demonstrated here.

The original attack on B's && was unfair: consuming A needs ownership too. Both need caller adaptation or copies; those copies also depend on FNVAL's still-open public copy contract. Removed the unconditional unchanged-callers benefit. C remains strong for owned helpers and small branches, but does not magically provide heterogeneous stored callbacks for a library API.

At-most-once is not exactly-once, sendability, lifetime extension or discharge of SingleUse. r4 requires valid zero-call cleanup, preserves input modes/effects and keeps crossing/cycle checks. Owned fields can have partial-move cleanup and reentrancy hazards; a runtime once flag or second stored-only wrapper is not an I8-compliant substitute.

### External alternatives

- Rust FnOnce consumes self, and is general over helpers, storage and thread bodies. Fn/FnMut implementations on borrowed repeatable functions preserve the original owner, but lifetime and Send requirements remain separate. Source: https://doc.rust-lang.org/std/ops/trait.FnOnce.html .
- Kotlin callsInPlace promises invocation only during the owner call, with an optional invocation count; it is not the complete stored/task contract. Source: https://kotlinlang.org/api/core/kotlin-stdlib/kotlin.contracts/-contract-builder/calls-in-place.html .
- Swift @escaping governs escape, not a static at-most-once allowance; closures can alias state and capture cycles. Source: https://raw.githubusercontent.com/swiftlang/swift-book/main/TSPL.docc/LanguageGuide/Closures.md , sections “Closures Are Reference Types” and “Escaping Closures.”
- C++ move_only_function separates move-only ownership from invocation count; it does not statically consume the function on each ordinary invocation. This is not equivalent to a one-shot callable.

Kotlin's closures documentation explicitly allows modification of captured outer variables: https://kotlinlang.org/docs/lambdas.html#closures . OCaml's create_counter example retains a mutable ref, while separate factory calls create independent state: https://ocaml.org/docs/mutability-imperative-control-flow . These strengthen FNVAL B and C respectively; neither language promises a deep snapshot merely by retaining another callable name. These are source comparisons, not foreign-language executed receipts.

## D-ARRAY-ZIPCOLUMNS1 — not ready

I agree with zero-element-copy descriptor adoption and checked borrowed views as coherent proposed operations. I do not agree with ratifying a performance-motivated public surface before its required built-candidate win. The measured adoption program is an ordinary record that B can use equally well.

### Independent measurements

Programs: `../probes/zipcolumns/{adopt,copy,rows,literal}/main.jet`, with each original package's counting allocator. Gate-p287, -O2, one compiler thread, sequential builds and runs under nice -n19. Outputs are `../probes/zipcolumns/review-r4/{adopt,copy,rows,literal}/main`. No compiler build or broad suite ran.

Each execution used a fresh Python process running the binary with subprocess.run. Wall duration uses time.perf_counter; child peak RSS uses resource.getrusage(RUSAGE_CHILDREN).ru_maxrss. Thus RSS is per fresh child, in KiB, not accumulated across earlier program runs. Timing includes process startup. This is a small three-run comparison on this workstation, not a universal ordering or compile-time benchmark.

| plain program | wall seconds, three runs | peak RSS KiB, three runs | allocations | requested bytes | allocator high-water bytes |
| --- | --- | --- | --- | --- | --- |
| consuming record | 0.650775 / 0.631017 / 0.643419 | 250096 / 250096 / 249960 | 12 | 256003288 | 256002896 |
| copying record | 0.775329 / 0.677947 / 0.660688 | 500028 / 502080 / 503976 | 20 | 512003472 | 512002896 |
| rows from columns | 0.559752 / 0.564175 / 0.561759 | 503980 / 503972 / 503972 | 16 | 512003380 | 512002896 |
| named-list literal | 0.491156 / 0.492385 / 0.491471 | 381092 / 381092 / 381096 | 13 | 384003288 | 384002896 |

All twelve executions print sum 10391999.750000002. Allocations and byte counts are identical across each program's three runs. Median wall times are respectively 0.643419, 0.677947, 0.561759 and 0.491471 seconds. Adoption has the smallest memory footprint, but is slower than rows and literal here. Even the author's old literal median beat its adoption median. Removed “fastest” and every implication that an actual Columns candidate was measured.

The consuming plain join is `fn(x: &&[Float], v: &&[Float]) -> Particles { Particles{x: x, v: v} }`. It already transfers descriptors without an element-copy loop. B is not forced to use the higher-memory named-list literal. The rows program starts from two parser columns and retains them while building rows; it is a fair same-input conversion cell, not evidence against direct row ingestion.

Taking ownership is not a buffer-uniqueness proof. The measured literal shares without an explicit .copy(); preexisting aliases do not vanish when another descriptor is taken. The join can add no element copy while a later write still incurs ordinary COW. r4 removes the false “every shared buffer is an explicit copy” claim and keeps the real detachment cost visible.

One buffer per field was already D-SOA-SITE1=A, not a newly engineered-away allocation loss. It permits adoption of separate buffers, but cannot also make those buffers one contiguous allocation. Reserve capacities to reduce growth work; do not silently change the ratified layout. I8 keeps the one Columns representation, not a second layout marker. D-ALLOC-FAIL1 still needs actual multi-field failure/cleanup proof.

Zero-field count, a user field named count, mismatch recovery, borrowed lifetime and move-only element loans are explicit in r4. Read-only access does not mean moving out unique elements. Removed the unproved allocation-free error assertion and the untested assumption that field windows can always be passed as owned lists.

Odin's documented soa_zip groups slices and soa_unzip recovers them: https://odin-lang.org/docs/overview/#soa_zip-and-soa_unzip . It is a useful borrowed-view comparison, not proof of Jet's owning recovery. The actual Columns candidate remains unbuilt and unmeasured in this review. AGENTS.md's pre-ratification gate cannot be moved into a future card criterion or relabeled an answered owner question.

## Exact redesign closure requirements

The names below describe **required future receipt probes**, not files that already exist or checks that passed. Each row is a concrete open item; closing it requires both the stated contract and the stated executable result. Existing negative witnesses and models remain evidence only for their recorded behavior.

### SERVICE-CURRENT1

| open item | probe that proves it closed |
| --- | --- |
| Payload-preserving admission must replace Actor.ask's unsent-envelope discard. | Extend the existing REVIEWR4 stopped-inbox witness to the proposed public call. With the inbox closed before submission, return the identical [unsent] and a non-Ambiguous admission error. Repeat with a unique cleanup owner; prove one owner and no leaked room permit. |
| Resolve, reserve and publish must coordinate with handoff, rollback, drain, partition and revocation, without blocking while holding registry coordination. | `service-admission-interleavings`: pause after route selection and after reservation; perform each route/state change, then resume. Every request must be admitted once to its permitted route or returned unchanged. Force a full mailbox and a handler performing service work; completion must not deadlock. Record the admission linearization point. |
| One identity resolver must support distinct delivery and maintenance policies. | `service-partition-maintenance`: retain one declared capability; partition, attempt delivery, reconcile, then deliver. Partitioned delivery returns its payload, reconciliation succeeds with that same identity, and foreign authority still fails. Include mailbox-depth, drain and worker-failure maintenance through the same resolver. |
| Durable admission and idempotency need an explicit commit point, retained-key scope and honest external-effect semantics. | `service-durable-commit`: stop immediately before and after journal/queue commit, then handoff or restart. Verify receipt identity, payload recovery only before commit, and documented duplicate suppression after retry. A handler with an external side effect must not be described as exactly-once without an atomic effect protocol. |
| Cancellation and deadline expiry need an ownership destination and exactly-once reservation cleanup. | `service-cancel-admission`: cancel before reservation, during reserved/unpublished state, and after publication. Use a counted cleanup owner. Verify legal cleanup or documented return before admission, no duplicated owner, no permit leak, and Ambiguous only when admission actually happened. |
| Transport replacement, discovery security and the public error amendment must be complete. | `service-restart-discovery-error`: replace the actor/process, revoke or expire signed discovery, and exercise old capabilities. No dead actor reuse or authority widening; freshness and pinning remain enforced. Compile callers matching R Refused<Q>! and move-only Q through Core exports and every public dispatcher, with the old R ServiceError! contract explicitly migrated. |

### FNVAL-COPY1

| open item | probe that proves it closed |
| --- | --- |
| Define static copy capability across erased fn signatures without forbidding unique repeatable owners or adding runtime copy refusal. | `fnval-erased-copy`: return a list-capturing callback and a unique-resource repeatable callback from separately compiled makers through their documented public signatures. Exercise function fields, Button, collections and forwarding. Eligible copies compile; the unique callback constructs, moves and runs repeatedly, but its copy fails statically at the correct public boundary. |
| Generate real callback capture copies and derive, rather than predict, the ownership oracle. | Run the actual `Tests/Fixtures/shared_owner_topology.jet` copy path under the candidate. Record all outputs and cleanup/strong-count transitions, including failed append. Compare independent Bucket mutation and shared owner retention, then update only the justified oracle lines. The explicit-record env-model is insufficient. |
| Preserve complete hidden-capture crossing and ownership summaries, including creator/task aliases. | `fnval-copy-crossing-cycles`: copy callbacks containing Shared, Cell, nested callbacks and unique owners, then transfer through a task and a callback field. Exercise the #6048 creator/task regression. Reject invalid crossings/cycles or serialize valid shared calls according to the single ratified proof; no fresh-environment exemption. |
| Establish copy cost, cleanup and failure behavior rather than claiming fixed counts or a deep snapshot. | `fnval-copy-cleanup`: mutate nested lists/maps after actual callback copy, retain shared state in both, then drop in both orders and fail between mutations. Verify independent ordinary contents, intentionally shared contents, exact cleanup and released owners. Report allocations/COW costs and exercise reentrancy, not just successful Int calls. |

### CALLBACK-ONCE1

| open item | probe that proves it closed |
| --- | --- |
| Consuming calls must use owned permission, not a read loan or a per-helper reset. | `oneshot-owner-transfer`: call a named OneShot through an owned helper once; reject a second helper call, renamed alias reuse, double forwarding and consuming invocation through an ordinary borrowed parameter. Branch-exclusive single calls pass; a possibly repeating loop fails. |
| Retained repeatable callers need a specified, sound scoped adaptation, including noncopyable owners. | `oneshot-repeatable-loan`: adapt one retained repeatable callback to two sequential helper calls, then call it directly. Repeat with a noncopyable captured resource, without copying it. Reject adapter escape/storage/task transfer beyond its loan and overlapping mutable calls. Do not allow that conversion to reset a genuine one-shot callback. |
| One callable contract must actually cover fields, returns, generic forwarding and task handoff. | `oneshot-places`: move a capture through a helper, a struct field, a returned OneShot and a task body; each produces the same result once. Reject second calls and invalid partial-field cleanup. Preserve F's taken/write inputs, effects and result through conversion, rather than erasing them. |
| Zero-call, error, panic and cancellation must preserve cleanup and SingleUse duties. | `oneshot-zero-cleanup`: use counted unique captures and a SingleUse duty; omit invocation on each exit path. Prove legal cleanup exactly once or reject construction when duty discharge is impossible. No silent dropping of a must-consume obligation. |
| I8 and crossing safety require migration of all special once positions into the same fact. | `oneshot-unified-crossing`: cover local direct call, Cell, Shared update and task positions with identical single-call/forwarding rules. Exercise creator/task aliases and captured-loan escape; reject unsafe crossing independently of call count. Show no helper allowlist, second wrapper contract or runtime reset flag. |

### ARRAY-ZIPCOLUMNS1

| open item | probe that proves it closed |
| --- | --- |
| Close the owner's performance question with the built Columns candidate before ratification. | `zipcolumns-paired-candidate`: run the actual candidate and the four recorded plain programs on identical parser outputs, single-threaded, same allocator and observable sum. Report individual runs, medians, peak RSS, allocations and bytes, plus compile cost if claiming it. The candidate must strictly beat the required wall-time and peak-memory baselines, including the consuming record; otherwise redesign or obtain an explicit owner change. No model or future criterion closes this item. |
| Prove owning transfer, existing-alias behavior, split and full recovery for unique fields. | `zipcolumns-ownership`: join with preexisting COW aliases, mutate, then split; verify aliases retain original values and any detachment cost is disclosed. Force mismatched lengths with counted unique elements; recover all original inputs and drop once. Include zero-field tables, invalid counts and a field named count, and verify count plus fields remain unambiguous. |
| Borrowed windows and failed growth need real lifetime, type and cleanup receipts. | `zipcolumns-loans-growth`: reject view escape, growth through a view and moving a unique element out of read access. Exercise actual field-window arguments. Inject failure at each column reservation/commit boundary after prior growth; values and lengths stay unchanged, no owner leaks or double drops, and both adopted and grown tables use the single ratified layout. |

## Final validation and posting disposition

All four r4 payloads report `ok` under gate-p287 Validate.jet, including the final ONCE marker-grammar correction. Payload schema acceptance is not design approval. Every adversarial verdict says Not ready. Do not post any of the four until its exact closure requirements above have executable receipts.

## Dispositions

<!-- audit-dispositions:v1 -->
| finding | disposition | target or reason |
| --- | --- | --- |
| D-SERVICE-CURRENT1 r3 not ready | card | #6048 holds the ballot; redesigned as r5 and reviewed again in ballot-redesign-r5-review-2026-11-18.md |
| D-FNVAL-COPY1 r3 not ready | card | #6548 holds the ballot; redesigned as r5 and reviewed again in ballot-redesign-r5-review-2026-11-18.md |
| D-CALLBACK-ONCE1 r3 not ready | card | #6780 holds the ballot; redesigned as r5 and reviewed again in ballot-redesign-r5-review-2026-11-18.md |
| D-ARRAY-ZIPCOLUMNS1 r3 not ready | card | #6177 holds the ballot; redesigned as r5 and reviewed again in ballot-redesign-r5-review-2026-11-18.md |
<!-- /audit-dispositions -->
