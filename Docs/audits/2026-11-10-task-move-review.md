# D-TASK-MOVE1 review: running one task at another place

Date: 2026-11-10. Author: BallotLead-3 (Claude Opus). Compiler: a0.0.43 (dev c460ba91e5).
Board: ballot D-TASK-MOVE1 on card #6281. Earlier pass: BallotFix REPORT.md, section D-TASK-MOVE1.

## Question

May a task whose function is top-level, and whose inputs and result encode, run at another place, checked
against that place's grants? Transport and identity come from D-SERVICE-IDENTITY1=D (ratified 2026-07-10)
and are not reopened.

## Findings that changed the ballot

1. **New join failures cannot go into TaskFailure.** TaskFailure is a closed, compiler-owned enum with three
   cases: Cancelled, DeadlineBlown and Panicked(String)
   (`Compiler/JetFoundation/Source/Registry/CoreEnumPayloadRows.jet:22-24`). Matches on it are checked as
   complete:
   - t04 covers all three cases and prints `ok 42`.
   - t05 drops `.Panicked` and gets E0307 NonExhaustiveDispatch, "missing: Err(.Panicked(_))".

   The board's "join gains PlaceLost and PlaceMismatch" would break every full match in local code. Today 32
   files name its cases, among them `Examples/Concurrency/ContextTaskInherit.jet:27-31`, which matches all
   three. A now gives a moved join its own PlaceFailure: `Task(TaskFailure)`, `Lost` and `Mismatch`. Local
   tasks are unchanged, and `join() ?? default` reads the same in both forms. "The same task handle" became
   "the same join and detach".
2. **The top-level rule excludes today's block tasks.** `task { … }` and `task &&owned { … }` exist
   (`Examples/Concurrency/FreezeCapture.jet:18-27`). The detail now says those cannot move and that captures
   become arguments.
3. **Rerun of the earlier findings.** t03's module-state hole still prints `bumped 5, hits 5`. Core still
   declares no Place type (a `git grep` of Core for a `Place` struct, enum or trait finds only
   `Core/Ui/Font.jet: PlacedGlyph`).

## Probes (a0.0.43; outputs in probes/out43)

| Probe | Result |
|---|---|
| t01 task on a plain value | `area 42, wire {"width":6,"height":7}` |
| t02 task on Shared | `hits 1` |
| t03 top-level function writes a module shared holder | `bumped 5, hits 5` |
| t04 full match on join failure | `ok 42` |
| t05 match missing Panicked | E0307 at 7:5 |

## Peers (sourced)

- Ray: "By adding the `@ray.remote` decorator, a regular Python function" becomes a task that runs on a
  cluster worker. <https://docs.ray.io/en/latest/ray-core/tasks.html>
- Unison: in distributed programs "dependency conflicts are impossible and there's no serialization code to
  write"; code moves with its dependencies. <https://www.unison-lang.org/articles/distributed-datasets/>
- Erlang: `spawn(Node, Fun)` starts a process on another node. <https://www.erlang.org/doc/apps/erts/erlang.html>

## Adversarial review (per claim)

| Claim | Attack | Verdict |
|---|---|---|
| join gains PlaceLost and PlaceMismatch | Is TaskFailure open? | **Failed**: closed and checked as complete (t05). Replaced by PlaceFailure |
| Same handle | Is the failure type the same? | Narrowed to the same join and detach |
| Top-level function, so no hidden captures | Block tasks capture | Holds, and the exclusion of `task { … }` is now stated |
| Module state rule | t03 through a helper | Holds: the check covers the whole call graph, like the effect row |
| Build identity | Skew | Holds: Mismatch refuses another build |

## Remaining losses

- A moved task pays encoding and network time.
- A full failure match on a moved task adds two arms.
- Closures and block tasks must be rewritten with their captures as arguments.

## Different-family review (GPT, 2026-11-10)

| Claim | Attack and independently checked evidence | Verdict |
|---|---|---|
| Local tasks and payload encoding work | Reran t01-t05: area 42 plus JSON, hits 1, hidden module hits 5, complete match ok 42, incomplete match E0307. | Holds locally only; no remote execution proved. |
| Encode means portable | Decoding, hidden process handles, nested duties and user codecs can break portability. | Fails; require both directions and recursive state/ownership checks. |
| Top-level plus direct-body inspection is enough | t03 reaches shared module state through a helper. Indirect calls and foreign edges may be unknown. | Fails; complete call-graph evidence or refusal required. |
| Compile-time grants are final authority | D-SERVICE-IDENTITY1=D names freshness, revocation and stale-generation outcomes. | Fails; revalidate using the one endpoint mechanism at dispatch. |
| Lost means work did not happen | A reply may be lost after remote side effects. Cancellation can also lose its acknowledgment. | Fails; explicitly report unknown outcome, no retry or rollback promise, and bound remote lifetime. |
| Task/Lost/Mismatch exhaust remote failures | The ratified service law also distinguishes partition, stale directory and revocation; codecs can fail. | Fails; preserve nested service and codec failures without changing local TaskFailure. |
| One-word change gives local semantics | Files and authority belong to the remote place; network failures and nested matches differ. | Fails; corrected to familiar operations, not equivalence. |
| B requires a service deployment per job | One typed job-runner can serve several job shapes. | Fails; strengthened B. |
| Closure payloads are necessarily hidden | An explicit capture list and payload inspection make them reviewable. | Fails; strengthened C. |

Read D-SERVICE-IDENTITY1=D and D-ACTOR1=A directly, and Ray and Unison sources independently. The earlier source census is not independently remeasured. No remote transport, grants, cancellation or build-mismatch claim is signed off as working today.

Agree with A only as an adapter over the existing service mechanism, with these failure and lifecycle obligations. No added option. Do not post transparent local/remote equivalence, exactly-once effects, or compile-time grants as permanent authority.
