# Fresh adversarial review: function copies and one-shot parameters

Date: 2026-11-17. Author family: Claude Opus. Reviewer family: GPT. Fresh reviewer: BallotReview2 (`BallotReview2-2`).

## Dispositions

<!-- audit-dispositions:v1 -->
| finding | disposition | target or reason |
| --- | --- | --- |
| FNVAL-COPY1 ready after repair | card | #6548 holds the posted ballot D-FNVAL-COPY1 |
| CALLBACK-ONCE1 ready after repair | no-action | posted on Tower as D-CALLBACK-ONCE1 for the owner to vote on |
| creator and task call one writing function value | card | #6048 criterion 5 makes the calls take turns |
<!-- /audit-dispositions -->

## Result

- D-FNVAL-COPY1: Ready (validator PASS), as `D-FNVAL-COPY1.r2.json`.
- D-CALLBACK-ONCE1: Ready (validator PASS), as `D-CALLBACK-ONCE1.r2.json`.

I agree with COPY A after its safety contract is repaired. I agree with ONCE A for synchronous helpers only. For arbitrary stored or asynchronous one-shot closures, ONCE B is the stronger solution; the ballot now says so plainly.

No fourth option was added. Both B options were substantially strengthened. COPY C now includes a complete, exercised explicit-state solution rather than an unspecified maker. ONCE C retains the exercised no-copy alternatives.

These are ballot-readiness verdicts, not claims that either proposed feature is implemented. Existing safety defects remain implementation acceptance gates, not authorized exceptions.

## Verification boundary

I read the repository contract and ballot skill, source and registry entries, the topology case and oracle, and the relevant ratified decision records. Tower and the repository were read-only. All new files are in the assigned scratch area. No compiler build or suite was run.

Jet claims below were checked with the supplied gate-p277 compiler. Proposed semantics are labeled PROPOSED or stated as requirements. In particular, no proposed function copy or #Once call was reported as running successfully.

Foreign-language comparisons were checked against the sources below. Rust, Swift, C++, Kotlin and OCaml compilers were not available on PATH. Accordingly, there are no independently executed peer receipts or performance claims. The reading surfaces now show a published Swift example and a published Rust signature excerpt, not invented peer execution results.

## COPY: findings per claim

| Claim or framing attacked | Finding and repair |
| --- | --- |
| Function copies and callback-containing aggregate copies fail today. | Confirmed by fc1, fc3 and fc9. Kept E0211, including the read-only-capture case. |
| A second binding shares automatically. | False for the exercised stored value: fc2 and fc4 report E0121. Kept ownership transfer distinct from explicit copying. |
| Borrowing an action shares its remembered state. | fc5 prints 303, 303. Kept this limited observation; it does not prove persistent copies work. |
| The topology fixture establishes working copy behavior. | It does not. Running the fixture stops at E0211 on line 72. Its literal oracle is a requirement, not a runtime receipt. |
| The oracle can simply be changed for another policy. | Removed that implication. B and C explicitly require owner-authorized changes of meaning; routine re-blessing is forbidden. |
| A changes no ratified meaning. | Misleading. D-COPY-WORD1 was a spelling-only amendment preserving the independent-value wording. A now explicitly amends that wording for function handles. |
| A copied mutable callback can rely on a new copy-count-based task rule. | Unsound and inconsistent with the existing invocation contract. Restored S47's shared invocation guard, preserved by D-FNVAL-WRITE1. |
| Moving a handle proves exclusive captured-state ownership. | False after aliasing. Crossings.jet accepts certain moved function types without examining captures. Require the canonical crossing proof over captures and aliases. |
| A read-only closure body is necessarily task-safe. | Not enough: it can call another captured callback or reach thread-confined state. Require transitive capture and crossing facts. |
| Refusing explicit copies removes the task race. | False. #6048 c5 covers the already accepted creator/task calls without an explicit copy. Every option retains that repair requirement. |
| A needs exactly one increment and no allocation. | Removed the blanket performance claim. Environment storage, atomic retain/release and guarded calls have costs. No measurements establish a fixed allocation count. |
| A can freely copy a local borrowing closure. | Only within its existing loan lifetime. Copying cannot extend a loan or bypass the escaping-capture checks. |
| A can make every function-bearing aggregate copyable. | Not when it hides consuming captures or invalid loans. Added copy eligibility through slots and aggregates. |
| Self-recursion refusal proves new cycles impossible. | False. Strong ownership paths can pass through Shared cells, nested callbacks, containers, helpers and erased values. Added those paths and later mutation to the existing cycle proof. |
| One E0221 rejection proves complete leak safety. | No. The callback-cycle probe exercises one refusal only. D-SHARED-CYCLE2 requires complete alias and call summaries; unknown reach must be refused. |
| The last alias has no user-visible cost. | Aliases extend capture and resource lifetimes. Named prompt handle release as mitigation; no fake claim that a copied resource is independently owned. |
| Guarding an environment has no reentry or contention cost. | Added same-environment conflict behavior, contention and task-wait lock cycles. Task-local makers and messages are the best mitigations; deadlock freedom is not promised. |
| A second maker call is a snapshot. | False. It normally restarts state and may replay effects. Use explicit snapshot data; an inaccessible or nonrecreatable maker is not a universal repair. |
| B must refuse copying after source erasure. | Unfairly weak. A generated environment-copy operation and propagated checked eligibility preserve known capabilities through fields and returns. |
| B requires every closure to be copyable even if nobody copies it. | Unnecessary restriction, removed during review. Noncopyable captures reject the copy, not formation or use of the original callback. |
| B yields completely independent effects. | False for Shared or other reference-like captures. It copies each capture according to its own law, not a recursive graph snapshot. |
| Rust permits only Copy captures. | Incomplete. Selective Clone is a serious stronger precedent. Kept Clone distinct from Copy and from Send/Sync. |
| C has no safety or runtime advantage. | Unfair. It avoids explicit function aliases and closure-copy retain traffic, although implicit task aliases still require repair. |
| C cannot perform the same two-menu job. | A complete explicit Shared-state factory prints 101, 202, 303 today. Its source/API restructuring cost remains real. |
| C incurs no retain operations anywhere. | Too broad. The explicit-state alternative copies Shared handles. Restricted the gain to closure-copy retain traffic. |
| Swift, C++, Rust, Kotlin and OCaml prove A universally superior. | Removed the popularity framing. Each supports useful sharing or independent-state patterns; none removes Jet's ownership, cycle or concurrency obligations. |

### COPY option costs and best mitigations

- **A:** Shared state is not a snapshot. Use explicit snapshot data when isolation is required. Aliases retain resources and guards may contend; scope handles tightly and use per-task makers or messages. Hidden capture facts can force refusal; publish checked summaries or expose explicit state. These losses remain visible.
- **B:** Copying noncopyable or unknown captures is refused; retain the original by move/loan or keep resource state external. Environment copying costs storage/work; use explicit shared state where intended. Reference-like captures still share; use owned snapshot data for isolation. Copy a stable source under its invocation guard.
- **C:** Callback aggregates stay uncopyable, and existing APIs may need factory/payload rewrites. An explicit Shared-state maker or dispatcher is the mitigation, not a promise that every inaccessible callback has a maker. Its policy does not waive the existing task bug.

The B topology sequence `103, 103, 203, 3, 1` is a **[INFERENCE]** from the fixture's arithmetic and per-capture copying, not a gate output. The r2 file labels it accordingly. A's `102, 202, 302, 2, 1` sequence is the unchanged literal oracle, also not a current gate output.

## ONCE: findings per claim

| Claim or framing attacked | Finding and repair |
| --- | --- |
| A helper calling once already accepts a moving capture. | False: on1 and on2b report E0121. Kept the distinction between visible implementation and an exported checked contract. |
| Existing local one-shot use proves general stored values work. | No. once-local prints 8 for direct local invocation only. No claim was made for returned or stored consuming function values. |
| Log is an owned file, so copying it demonstrates file duplication. | The probes use a string-buffer struct. Corrected the story. on5's copy workaround does not solve noncopyable resources. |
| At-most-once means exactly once. | False. Zero-call paths are legal, with capture cleanup and separate mandatory-duty checks. |
| At-most-once implies nonescape. | False. A now explicitly promises synchronous, nonescaping use. Its prohibition on task handoff is a design change, not a derived fact. |
| A task handoff is safe merely because it moves the callback. | False. Invocation count does not establish Send-like capture safety or sufficient lifetimes. A forbids task handoff; B requires normal crossing checks. |
| #Once can replace ownership transfer. | That would conflict with D-MOVE-SIGIL1 and add overlapping meanings. A now requires `#Once done: &&fn(...)`; && transfers ownership, while the mark constrains use. |
| Adding the mark preserves all callers unchanged. | Not with an owned parameter. Retaining callers need an explicit repeatable copy where available, or external state. Required a complete helper/caller migration. |
| Once is already registered. | marker-once reports E0927. No proposed #Once success is presented as a receipt. |
| Syntax/Markers.jet is a second registry of marker names. | False. Its comments designate Prelude/Markers.jet as the marker source. Register Once once, at Parameter, with no arguments. |
| Existing parameter grammar already handles a registered mark generically. | False. Items.jet specially handles one prefix marker and Root. Registration alone is insufficient. Required AST, signature, sema, formatter and tooling retention. |
| The mark can disappear through a function value. | Losing it loses the usable promise; falsely adding it is unsafe. Specified retention in inferred/exported/nested signatures and only safe promise-erasing conversion with unchanged access modes. |
| A renamed local can regain permission, or copying an aggregate can duplicate it. | No. Track one permission with ordinary ownership flow and forbid copying or repeatable erasure. Consume permission before entering user code. |
| Branches, loops and nested callbacks need no special treatment. | Added disjoint-path checking, proof for any loop allowance, checked nested nonescaping use, forwarding transfer, and reentry checks. |
| Uncalled captures can always just be dropped. | Not mandatory SingleUse duties. The existing marker means a job must be done exactly once. Reject construction unless every path discharges required duties. |
| A needs a new private helper-name once table. | Violates I8. Generalize existing once-call facts and migrate synchronous helper exceptions in one cutover. Task bodies reuse that fact without gaining a parameter escape. |
| Rust FnOnce alone makes thread spawning safe. | False. spawn separately requires Send and a sufficient lifetime. Kept invocation count and crossing safety independent. |
| Swift escaping means one-shot. | False. It permits use after return, without constraining call count. |
| Kotlin has only a count promise. | Incomplete. callsInPlace also specifies invocation during the owning call; AT_MOST_ONCE separately permits zero or one call. |
| C++ move_only_function with an && call operator consumes the wrapper. | False. Its specified operator invokes an rvalue target; it does not empty the wrapper or statically prevent a second invocation. Corrected both prose and comparison. |
| B inherently allocates, dispatches dynamically or needs a runtime used flag. | Unsupported. A generic inline representation need not allocate; erased storage may. Removed invented unavoidable costs. |
| B is merely for a rare deferred callback case. | Unfair. Arbitrary stored completions, returns, cancellation and task ownership are major expressive advantages. B is now a serious consuming-value option. |
| A Core wrapper around today's fn can legalize moving captures by itself. | False. B explicitly requires contextual compiler support and the same consuming-call fact, not an ordinary repeatable inner function. |
| Payload plus function is an equivalent replacement for every stored closure. | False. It can expose heterogeneous captured types and force API redesign. Kept it only as a mitigation with that cost. |
| C is a copying-only workaround. | False. on3b and on6 both print 8 without copying. Preserved these strong explicit-state and branching alternatives. |

### ONCE option costs and best mitigations

- **A:** Stored arbitrary one-shot closures and task handoff remain unavailable. Typed payloads/messages mitigate some jobs but change interfaces; select B for the general stored-value requirement. Owned parameters can break retaining callers; migrate them together. Explain rejected second-call and escape paths with concrete repairs.
- **B:** Broader compiler/representation commitment and explicit wrapper/call-method spelling. Use the same consuming-call checker and ordinary ownership rules, not an independent runtime flag. Keep construction explicit and show the call method in diagnostics. Do not ship both A and B as parallel one-shot mechanisms.
- **C:** Wider payload types, helper signatures and branch propagation. Encapsulate typed payloads or move branching beside the owner. Copying remains optional for copyable buffers, never a general resource remedy.

The recommendation remains A only for the clearly stated synchronous scope. I do not endorse describing it as the universal solution to arbitrary one-shot callbacks.

## Jet execution receipts

Every row below was independently compiled and run, once per source program, with the supplied gate compiler. Successful output is listed in order. Failure rows list the observed diagnostic, not a claim that the program ran.

| Program, relative to new/ unless stated | Exit | Observed result |
| --- | ---: | --- |
| probes/fnval-fc1.jet | 1 | E0211, function copy |
| probes/fnval-fc2.jet | 1 | E0121, transferred callback used again; copy suggestion is misleading |
| probes/fnval-fc3.jet | 1 | E0211, read-only Int capture still not copyable |
| probes/fnval-fc4.jet | 1 | E0121, transfer into aggregate |
| probes/fnval-fc5.jet | 0 | 303, 303 |
| probes/fnval-fc6.jet | 0 | 101 |
| probes/fnval-fc7.jet | 0 | 11, 12 |
| probes/fnval-fc8.jet | 0 | 102, 201; existing #6048 c5 defect, not evidence of safe synchronization |
| probes/fnval-fc9.jet | 1 | E0211, Button copy |
| probes/fnval-fc10.jet | 0 | 101 |
| probes/once-on1.jet | 1 | E0121, moving capture through ordinary helper |
| probes/once-on2b.jet | 1 | E0121, moving capture through optional map |
| probes/once-on3b.jet | 0 | 8 |
| probes/once-on5.jet | 0 | 8, 1 |
| probes/once-on6.jet | 0 | 8 |
| review/marker-once.jet | 1 | E0927, unknown applied rule Once |
| review/once-local.jet | 0 | 8 |
| review/cycle-callback.jet | 1 | E0221, callback captures its destination Shared cell |
| review/explicit-state.jet | 0 | 101, 202, 303 |
| repo Tests/Fixtures/shared_owner_topology.jet | 1 | E0211 at line 72 |

The known fc8 defect is already owned by #6048 c5. Its particular printed interleaving is not a timing guarantee or a dynamic race-detector result.

Commands used the supplied environment:

```sh
export PATH="$JET_STATE_ROOT/lander3-tmp/gate-p277/tree/Compiler/JetCLI/.jet/build:$PATH"
export NO_COLOR=1
export JET_TOOLCHAIN_ROOT="$JET_STATE_ROOT/lander3-tmp/gate-p277/tree"
jet run <program.jet>
jet "$REPO/Tools/Agent/Ballots/Validate.jet" D-FNVAL-COPY1.r2.json D-CALLBACK-ONCE1.r2.json
```

Final payload-only validator receipt, after the last edits:

```text
ok D-FNVAL-COPY1.r2.json (D-FNVAL-COPY1)
ok D-CALLBACK-ONCE1.r2.json (D-CALLBACK-ONCE1)
```

Exit: 0. Earlier validation caught unescaped paragraph breaks in the newly written JSON and an overlong ONCE check paragraph; both were repaired. Validation is not an implementation, runtime, concurrency or performance proof.

## Sources checked

### Jet

- `Tests/Cases/RunRegressions/shared_and_closure_owner_topology_is_preserved` and its fixture/oracle relationship.
- `Tests/Fixtures/shared_owner_topology.jet:22-28,70-77`: captured Bucket plus Shared owner and the explicit function copy.
- `Tests/Cases/RunRegressions/shared_owner_topology.out:10-14`: literal topology sequence.
- `Docs/Spec/spec.md:1657-1695`, “Captures and escaping lambdas”: intended capture law and special run-once positions, not evidence of current completeness.
- `Docs/Spec/spec.md:2084-2088,4137-4146`: copy-law wording and task capture/crossing contracts.
- `Docs/Spec/syntax-decisions.md:668-690`: S47, especially invocation locking at 687-690.
- `Compiler/JetSema/Source/Sema/Ownership/Crossings.jet:307-341,471-488`: function crossing checks and the insufficient moved-function shortcut.
- `Compiler/JetSema/Source/Sema/Ownership/Checks.jet:129-158`: existing once-call facts.
- `Compiler/JetSema/Source/Sema/Ownership/Copy.jet:1370-1371`: copy diagnostics.
- `Compiler/JetSema/Source/Sema/SharedCycles.jet:12-18,80-98,197-200,319-334,436-442`: hidden callback paths, source-copy reach and cycle diagnostics.
- `Compiler/JetFoundation/Data/Syntax/Markers.jet:4-6`: one marker source.
- `Compiler/Prelude/Markers.jet:388-391,692-696`: SingleUse's Type site and Root's Parameter site.
- `Compiler/JetParser/Source/Parser/Items.jet:876-887`: parameter-marker handling.
- `Compiler/JetSema/Source/Sema/Ownership/SingleUse.jet:1-15`: exactly-once duties, not optional invocation permission.
- Read-only Tower decision records: outcome A and ratified status checked for D-COPY-WORD1, D-FNVAL-WRITE1, D-SHARED-CYCLE2, D-CONC-CROSS1, D-CALLSITE-MARK2 and D-MOVE-SIGIL1.
- Read-only card #6048 c5: accepted creator/task mutating-callback race and required safe refusal or synchronized state.

### Existing-language solutions

- [Rust closure call traits, Clone/Copy and Send/Sync](https://doc.rust-lang.org/reference/types/closure.html#call-traits-and-coercions): consuming versus borrowed calls, with separate capture-derived traits.
- [Rust Option::map](https://doc.rust-lang.org/std/option/enum.Option.html#method.map): `F: FnOnce(T) -> U`.
- [Rust thread::spawn](https://doc.rust-lang.org/std/thread/fn.spawn.html): `FnOnce + Send + 'static`, with separate output bounds.
- [Swift book, Closures](https://raw.githubusercontent.com/swiftlang/swift-book/main/TSPL.docc/LanguageGuide/Closures.md): reference-type closures, aliasing counter example, escaping and reference-cycle warning.
- [Kotlin lambdas and closures](https://kotlinlang.org/docs/lambdas.html#closures): first-class functions and writable captured outer variables.
- [Kotlin callsInPlace](https://kotlinlang.org/api/core/kotlin-stdlib/kotlin.contracts/-contract-builder/calls-in-place.html): invocation during the owning call.
- [Kotlin AT_MOST_ONCE](https://kotlinlang.org/api/core/kotlin-stdlib/kotlin.contracts/-invocation-kind/-a-t_-m-o-s-t_-o-n-c-e/): zero or one invocation.
- [C++ std::function constructors](https://en.cppreference.com/w/cpp/utility/functional/function/function): copies target and requires copy-constructible targets; storage and throwing-copy costs are not universal constant costs.
- [C++ shared_ptr copy construction](https://en.cppreference.com/w/cpp/memory/shared_ptr/shared_ptr): copied handles share ownership, so copied targets need not isolate referenced state.
- [C++ move_only_function invocation](https://en.cppreference.com/w/cpp/utility/functional/move_only_function/operator()): rvalue invocation casts the stored target; it does not consume the wrapper.
- [OCaml values, bindings and closures](https://ocaml.org/docs/values-and-functions) and [mutable references](https://ocaml.org/docs/mutability-imperative-control-flow): first-class functions and explicit mutable cells, not a consuming-call type contract.

## What must not be posted

Do not post either original draft. In particular, do not repeat the C++ consuming-wrapper claim, source-erasure straw man, universal independent-state claim, file-copy story, or the claim that registering Once alone implements its grammar.

Do not call the proposed designs implemented, allocation-free, universally superior, race-free today, or complete for arbitrary one-shot storage. Do not treat the topology oracle as observed current execution. Do not waive #6048 c5, the full cycle proof, existing invocation guards, or cross-tier acceptance.

The r2 ballots can be posted as explicit choices with their named costs and mitigations. Building either direction still requires its stated end-to-end acceptance checks.
