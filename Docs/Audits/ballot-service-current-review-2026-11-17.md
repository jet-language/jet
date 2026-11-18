# D-SERVICE-CURRENT1 adversarial review
<!-- audit-dispositions:v1 -->
| finding | disposition | target or reason |
| --- | --- | --- |
| D-SERVICE-CURRENT1 ready after repair | card | #6048 holds the posted ballot D-SERVICE-CURRENT1 |
| a stale-refused call consumes its message | card | #6048 carries the ballot, which names this loss and its mitigation for the owner's vote |
| durable feeder keyed by declaration generation | card | #6048 criterion 8 keys the feeder by worker at its current generation |
<!-- /audit-dispositions -->

Reviewer: GPT, fresh agent BallotReview2 (runtime identity BallotReview2-2). Author: Claude Opus. Date: 2026-11-17.

Reviewed `D-SERVICE-CURRENT1.json` and `D-SERVICE-CURRENT1.review.md`. Repaired payload: `D-SERVICE-CURRENT1.r2.json`. No repository or Tower changes. Probes use gate-p285 and a scratch copy of current dev `Core/Service/Service.jet` (read snapshot FE4D, 6,355 original lines).

## Verdict and recommendation

Ready as a choice of public semantics, subject to payload validation below. I agree with A for **explicit route snapshots without directory setup**, not because B breaks ratified identity rules or C requires runtime type identity. Those arguments were wrong. B and repaired C are genuine, safe competing designs under their stated trust and routing checks.

This is not implementation approval. Public `tree.current`, typed registration, and the witness overload were not wired by this review. The ballot names implementation acceptance gates rather than calling a scratch prototype production-ready.

## Findings per claim

1. **“After handoff the typed address is stale.” Holds for the exercised unpinned route.** Independently ran the author's unchanged `probes/current-stale-today.jet`: before 10, handoff 2, then `service endpoint shop/api@g1 is stale (current generation 2)`. Pinned routes are an explicit exception. The repaired text says route generations, not changing message types.

2. **“Nothing is lost or doubled; refresh and resend is safe.” Fails.** `call` takes `message: &&Q` at Core/Service/Service.jet:5301 and checks the route inside the callee through `typed_queue`. Argument ownership already transferred. `owned.jet` independently gets E0121 when it reuses an explicitly moved list after this stale call. Prequeue rejection proves no admission for that attempt, not retention of the original payload, exactly-once effects, or reusable unique resources. Arguments can also have effects before queue validation. Removed every unconditional lossless-retry and exactly-once claim, including the surface lesson, gains, mitigation and design-away list. Best mitigation: refresh before constructing the request after a known upgrade; retain reconstructable request data when retries matter. A unique moved resource is not recovered by this API. Never automatically retry Ambiguous.

3. **“Refreshing never returns a route that would be stale.” Too broad.** A snapshot can be correct when read and stale when used after a subsequent handoff. Refresh and call are separate operations, not a transaction. Removed the guarantee. Named a possible second Stale, explicit recovery and route-snapshot synchronization. No timing or contention advantage is claimed.

4. **A follows handoff, rollback, pin and reconciliation. Holds for the internal prototype.** Independent probes print g2/30 after handoff, g1/50 after rollback, pinned g1 with Partitioned, then reconciled g2/30. A can return metadata for a drained worker; call still refuses NotStarted. These receipts establish the proposed Core function, not a working public method or global distributed atomicity.

5. **“Same token plus private fields means no new authority.” Incomplete proof.** The prototype rejects a foreign authority even when its tree and worker names match. But private fields do not establish signed discovery expiry, revocation, synchronized generation reads, or valid transport after restart. Made those explicit acceptance requirements. Refresh must never turn name equality into a fresh authority grant. The recommendation is a local same-worker capability update, not a second discovery/trust mechanism.

6. **“Journal restoration gives the current typed address.” Holds only with a valid new transport witness.** Independently exercised new-tree declaration, the same journal, restored g2, and C returning the same id as A with answer 60. A rejects the old tree's endpoint as foreign. Restoring ids is not restoring a live old process actor. The repaired design says newly declared or validly discovered witness, not revival of a dead transport.

7. **B “narrows D-SERVICE-IDENTITY1.” Fails as stated.** Read the ratified decision's D outcome from the Tower JSON/history read-only. It requires typed declared capabilities, signed directory snapshots, directory generations, maximum staleness, trust and explicit failure. Those directory generations are not identical to an internal worker incarnation. A stable logical reference can preserve them while resolving the current allowed route. Rewrote B to preserve expiry, revocation, partition pinning, compatible migration, explicit failure and no retries after uncertain admission. Its real cost is changing today's route-snapshot behavior and losing Stale as a local handoff notification. Existing rollout receipts mitigate that cost. B must still synchronize routing and rebind actual transports correctly.

8. **B “always reaches the current worker” and “never Stale.” Misleading.** Stable logical identity does not imply availability or override stale discovery. Removed both absolutes. B removes Stale solely for a local handoff, not all failure modes. Ratified migration compatibility remains mandatory; the original appeal to changing expected message types was not a justified disadvantage of logical references.

9. **C “needs erased values, runtime type ids and a new journal format.” Fails for a strong omitted design.** Added `tree.directory_resolve("price", api)`: existing signed lookup produces an id; a retained `Endpoint<Q,R>` is the type witness. Require authority, tree and worker equality, ignoring route generation only. Reuse the witness's typed actor and semaphore. No mixed typed values enter the directory, no caller-asserted cast is needed, and no journal field changes. This independently compiled and ran with Int and String workers. A wrong-worker witness is refused by identity. C preserves compile-time message types just like A.

10. **C can discover any typed worker from a name alone. Not claimed after repair.** The repaired C deliberately needs an existing witness. If an alias is rebound to another worker, even a same-signature worker, it refuses that witness. Mitigation: explicitly acquire that worker's declared or trusted typed capability. Alias registration and retained witness are real costs; registering once during setup mitigates setup work, not general discovery limitations. Arbitrary bare-name typed discovery is outside this choice.

11. **C and A have identical availability behavior. False; now explicit.** Existing `directory_resolve` validates proofs, authority, running and draining. Its prototype reports Partitioned on a partitioned alias and refuses a drained alias; A can still return metadata, with later call refusal. C callers wait for valid resolution; A must not promise availability. These differences are in long form and the reading surface.

12. **Erlang requires `whereis` after restart. Unfair comparison.** Official `gen_server` docs accept a registered name directly as ServerRef, including local/global/via forms. The strongest alternative avoids explicit refresh. Replaced the example with source-labeled `gen_server:call(price, ...)`, without claiming an Erlang execution or compile-time message types. Failure and timeout remain possible.

13. **Akka Typed proves runtime erasure is needed in Jet. Unsupported.** Official documentation shows typed ServiceKey[T], receptionist listings and subscriptions. That supports typed discovery as a strong alternative, not a requirement that Jet serialize runtime type identity. Removed the type-erasure argument and kept the typed-key comparison.

14. **Orleans references “always reach” and callers “never see staleness.” Unsupported absolutes.** Official documentation supports logical interface/key identity independent of activation location, including restart. It does not prove universal reachability, absence of failures or exactly-once delivery. Corrected both long form and comparison.

15. **A wins because it is an eight-line function. Invalid ranking.** Removed implementation size and difficulty as reasons to select a public API. A wins only if explicit route snapshots and avoiding alias setup fit the owner's intended semantics. All options need complete routing, trust and compiler integration.

16. **“27 cases” and universal one-line migration. Not independently established.** Removed the externally supplied precise count and claim every migration becomes the same safe retry. Typed WORKER1 and REPLY1 cutover still applies to every caller. Resource ownership, handler-based observation and former `receive` uses require proper migration. No old String messaging compatibility shim is authorized.

17. **A also authorizes typed `.show()` and typed directory registration. Unnecessary bundled surface.** Removed typed display from A's authorization and public example. C explicitly needs typed registration and witness overload wiring; its scratch probe uses a private id inside Core. A does not need directory registration. A's generic source export, signature, lowering and dispatcher integration remain explicit requirements.

18. **“Key the feeder by worker” fixes durable handoff. Incomplete and unsafe as a standalone prescription.** Source inspection verifies generation-bearing registration and exact endpoint comparison at 1251–1264. `feed_open` independently indexes the declaration endpoint's generation at 1268–1272; `feed_start`/`feed_wait` keep that id at 5184–5198. Merely waking by worker cannot repair stale reads or preserve generation fencing. Replaced with an end-to-end transition criterion including pending receipts, rollback, pinned workers, authority and restart. The author already observed a delivery failure; I did not rerun it to confirm their report. Removed their timing and success-count measurements as independent evidence.

19. **Tasks and I1 safety. Not waived by any address policy.** Endpoint copies share an actor and semaphore, so rebuilding ids must never fork state or bypass admission. Worker handlers still require exclusive state access. The already tracked task/creator closure race on #6048 remains a blocking implementation criterion, not a benefit of refresh or proof of safe concurrent callbacks. This review does not claim a race-free implementation from sequential probes. A needs synchronized route snapshots; B must resolve under authority routing; C must validate proofs and witness identity without unchecked casts.

20. **I8 and marker grammar.** None of A, B or repaired C introduces a marker, special message syntax, or a second authority mechanism. Read `Compiler/JetFoundation/Data/Syntax/Markers.jet`: prefix syntax and marker-name ownership remain unchanged. API integration belongs in the existing generic service machinery. Verified existing owners at `Compiler/JetFoundation/Source/Registry/CoreValueSignatures.jet`, `Compiler/JetSema/Source/Sema/Calls/Services.jet`, and `Compiler/Prelude/Core.rows` (not a root `Prelude/Core.rows`). C reuses signed resolution; A is local snapshot refresh; B offers one logical Endpoint type, not parallel handle kinds.

## Ratified constraints checked

Read D-SERVICE-IDENTITY1 D, D-SERVICE-UPGRADE1 D, D-SERVICE-WORKER1 A and D-SERVICE-REPLY1 A from `$HOME/Projects/Github/jet/Tools/Tower/.tower/tower.json` and decision histories, read-only. Also checked KEEP1 and BATCH1 when interpreting handler and state rules. No new decision was written. In particular:

- Directory generation and maximum staleness survive all three choices.
- Partitioned shards retain allowed generations and use rollout reconciliation.
- Compatible endpoint/state migrations remain required; automatic routing does not license changing a typed interface.
- `call` returns typed R or ServiceError; Ambiguous does not imply no execution.
- String messaging and `receive` are removed by the existing typed worker decision, not preserved here.

## Independent receipts

Compiler: `$HOME/.cache/jet-dev/lander3-tmp/gate-p285/tree/Compiler/JetCLI/.jet/build/jet`.

Current behavior:

```
JET_TOOLCHAIN_ROOT=<gate-p285 tree> NO_COLOR=1 jet run new/probes/current-stale-today.jet
before:10
handoff:2
after:service endpoint shop/api@g1 is stale (current generation 2)
```

Scratch prototype source: `review/current-r2/tc/Core/Service/Service.jet`. The appended `review_current`, `review_resolve` and three `REVIEWCURRENT` probes are review-only code, not exported methods. Other toolchain sources are symlinks to gate-p285. All scratch writes, including the journal, remain under the permitted review directory.

```
JET_TOOLCHAIN_ROOT=<review/current-r2/tc> NO_COLOR=1 jet test <scratch Service.jet> --filter=REVIEWCURRENT --capture=all
before:10
old:service endpoint review-shop/api@g1 is stale (current generation 2)
A:Endpoint(review-shop/api@g2):30
C:Endpoint(review-shop/api@g2):40
C-mismatch:directory entry is not the witnessed worker
rollback:Endpoint(review-shop/api@g1):50
foreign:foreign endpoint
pinned:Endpoint(review-pins/api@g1)
partition:service endpoint authority is partitioned
C-partition:service endpoint authority is partitioned
rejoined:Endpoint(review-pins/api@g2):30
drain:worker `api` is not running
C-drain:directory entry `price` no longer names a running worker
journal:Endpoint(review-journal/api@g2):60
old-authority:foreign endpoint
3 passed, 0 failed, 0 skipped
```

Probe development first rejected returning a borrowed String as-is (E0120); corrected the String handler to return `n.copy()`. The final three-probe run above includes the heterogeneous worker and journal case. No build or broad test suite was run.

`review/current-r2/owned.jet`, run with unmodified gate-p285, rejects reuse of `message` with E0121: “message was consumed by core.service.call.” This is a deliberate negative ownership receipt, not a failed test to suppress. Runtime stale refusal is independently shown above. No foreign-language program was compiled; external examples are explicitly source illustrations, not execution claims.

## Sources read

- Erlang/OTP `gen_server`: https://www.erlang.org/doc/apps/stdlib/gen_server.html#call/2 — `server_ref()` and `call/3`, including registered-name references, timeout and noproc failures.
- Akka Typed actor discovery: https://doc.akka.io/libraries/akka-core/current/typed/actor-discovery.html — ServiceKey[T], Listing, subscriptions and ActorRef[T] sets.
- Orleans grain references: https://learn.microsoft.com/en-us/dotnet/orleans/grains/grain-references — logical interface/key identity independent of activation location and restart.
- Repository `AGENTS.md:404-429`, ballot skill, syntax marker registry and Core/compiler anchors cited above were read directly. No source line references in this review are inferred from prose.

## Posting exclusions

Do not post the original draft's lossless retry guarantee, claim B inherently violates IDENTITY1, runtime-type-identity objection to C, unconditional Orleans reachability, or eight-lines-as-superiority argument. Do not post a claim that public methods, signed expiry enforcement, concurrency safety or durable handoff are implemented and passing. The revised ballot authorizes a choice and names implementation gates; it does not discharge them.

## Payload validation

Ran gate-p285 `jet Tools/Agent/Ballots/Validate.jet new/D-SERVICE-CURRENT1.r2.json` using absolute paths and the unmodified gate toolchain root. Result: `ok $JET_SCRATCH_ROOT/lanes/Main/ballot-rework/new/D-SERVICE-CURRENT1.r2.json (D-SERVICE-CURRENT1)`. Ready (validator PASS).
