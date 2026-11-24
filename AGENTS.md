# AGENTS.md — Jet agent contract

This is Jet's single policy for every agent and tool. It covers authority,
documentation, invariants, the performance gate, work state, delegation, and
proof. Strategy lives in `Docs/Spec/philosophy.md`. Plans and work state live
only in Tower. Procedures live in skills. Current behavior is established by
code, executable registries, and exercised tests and examples.

## Authority

Read `Docs/Spec/philosophy.md` for Jet's purpose and ranked design priorities.
Do not copy that guidance into other documents.

Code is the source of truth for what Jet does. Specs explain durable contracts
and their reasons; they do not prove that a feature works, is missing, or is
complete. When code conflicts with an approved requirement, treat it as a
defect: investigate it and record it in Tower. Do not bless the implementation
or repeat stale prose.

When rules about intended behavior or agent conduct conflict, this order
decides:

1. The owner's latest explicit instruction.
2. A ratified Tower decision and its acceptance terms.
3. The relevant domain specification or ADR.
4. This contract.
5. The task-specific skill.

A newer ruling from a higher authority wins. Never average conflicting rules.
When a conflict or owner gate blocks one slice of work, stop only that slice
and continue with independent work.

## Verify before claiming or changing

No agent may state a claim about Jet's syntax, semantics, shipped features,
missing features, diagnostics, docs, or Tower state from memory or from
intuition about another language. Before reporting to the owner, filing a card
or ballot, or editing code or prose:

1. **Read the syntax registry first.** Check
   `Compiler/JetFoundation/Data/Syntax/Spellings.jet` (and sibling files under
   `Compiler/JetFoundation/Data/Syntax/`) before naming any Jet keyword, sigil,
   marker, or grammar form. Never write another language's syntax as Jet syntax
   or claim a registered spelling does not exist.
2. **Read the spec and live code together.** Check `Docs/Spec/spec.md`, the
   relevant `Docs/Spec/Reference/` page, and the implementing compiler or Core
   source before claiming a feature is missing, partial, or broken. Distinguish
   explicitly between (a) a defect or unbuilt piece of an already-ratified
   decision (cardable without a ballot) and (b) a genuinely unratified surface
   change (ballotable only after verifying no decision already covers it).
3. **Check Tower before planning, carding, or balloting.** Search existing
   cards and decisions in Tower first. Attach defects and follow-ups to the
   existing owning card; never duplicate a card or re-ballot a settled choice.
4. **Cite the exact source.** Every claim about what Jet allows, forbids, or
   ships must rest on a file, symbol, registry row, or command output read in
   the current session. Guessing and filling gaps with plausible prose is a
   contract violation.

## Decision levels

| Level | Decides | Central question |
|---|---|---|
| **Strategic** | Purpose and direction: desired outcomes, priorities, boundaries, trade-offs, and what success means. It may question whether work is worth doing at all. | What are we trying to accomplish, and why? |
| **Operational** | The approach: what must exist or change, how the parts fit, sequencing, scope, dependencies, milestones, and acceptance criteria. | What approach and body of work will achieve the outcome? |
| **Tactical** | Concrete actions: implementation details, specific changes, immediate problems, and verification. | What exactly do we do, and how do we show it worked? |

Tactical findings can reveal that an operational approach or a strategic
assumption needs to be revisited. Raise them; do not silently absorb them.

## Documentation

- **One strategy document.** `Docs/Spec/philosophy.md` holds the durable vision
  and design priorities. It never holds schedules, feature inventories,
  progress notes, or release scope. Do not start a separate roadmap without
  owner direction.
- **One planning system.** Tower holds epochs, plans, milestones, dependencies,
  decisions, acceptance criteria, status, blockers, and handoffs. Move live work
  into Tower before deleting a document copy. Discard superseded plans instead
  of keeping a second queue.
- **Executable truth.** Spellings, APIs, diagnostics, defaults, and constraints
  live in their code or registry home. Tests, snapshots, and executable
  examples prove behavior. Generate reference output on demand; never commit
  generated status or duplicate catalogs into docs.
- **Short explanations.** Specs and guides explain durable contracts,
  non-obvious reasons, and how to use or change the system. Link to executable
  sources instead of maintaining feature lists, limitation lists, coverage
  tables, or implementation maps. Fix stale prose by cutting or correcting it,
  never by redefining reality.
- **Evidence is not work state.** Audits and research keep dated findings.
  Proposals keep alternatives and reasons tied to a Tower decision. None of
  them may own a plan, goal, or live status. Historical evidence never
  establishes current behavior.

`Docs/` has four document categories: `Spec`, `Audits`, `Research`, and
`Proposals`. `Docs/README.md` is navigation only; `Docs/Site` is the generated
project website and `Docs/Assets` holds images. Only the owner may delete
audits or final reports; agents may move them intact. Do not create cleanup
reports, preservation maps, archive folders, or recurring documentation
janitors. Keep unique decision rationale where it already lives, and use Tower
and source history for work tracking and recovery.

## Repository layout

| Path | Holds |
|---|---|
| `Compiler/` | The Jet compiler (native backend, runtime, embedded Prelude under `Compiler/Prelude/`) and its bootstrap assembly |
| `Jetpack/` | The Jet package manager |
| `Core/` | Core library sources |
| `Examples/` | Executable examples and golden output |
| `Tests/` | Native test corpora (cases, UI snapshots), fixtures, and compiler proofs |
| `Docs/` | Documentation, the website, and assets |
| `Tools/` | Contributor tooling: `Agent/` (environment and gates), `CI/`, `Perf/`, `Gauntlet/`, `Editors/`, `tower/`, and more |

Every folder is TitleCase and writes acronyms in capitals (`Tests/`,
`Compiler/JetCLI/`). Jet module folders and data trees keep the names their
tools require; `Tests/Guards/acronym_path_case.jet` enforces the rule. Keep the top level small: new tooling goes under
`Tools/`, new documentation under `Docs/`. Never commit build output, scratch
files, or one-off run artifacts.

## Tooling in other languages

Repository-management tooling is Jet: the scripts under `Tools/Agent/`,
`Tools/Release/` and `Tools/CI/`, which hold the landing, chain, census,
lane-check and release gates and generators. Everything else (editor
extensions, the gauntlet, perf, packaging, agent-eval, peer programs, test
and benchmark subjects) stays in the language its host needs and is out of
scope. New tooling in scope may be written first in another
language and proven working; then its Jet twin is written, in the best Jet
that does the job rather than a copy of the original's exact output, and
proven to do the same job. The pair is a gauntlet entry: an entry under
`Tools/Gauntlet/Entries/` with `"mode": "tool"` names the original as
`tool.original` and the twin as `tool.jet`, and holds the shared fixtures and
the parity cases with their expected output; a case's `check`, a Jet program
in the entry, prints the facts the job leaves (the files, commits and queue
lines it produces, the facts its report states), and both programs must give
the case's exit status and that expected print. The repository keeps running
the original until the Jet twin passes the port bars (#5072 c14, c34).
Performance ranks first: the same job, lower wall time and lower peak memory
decide, with the reasoning bar beside them (no harder to reason about,
scored on a fixed checklist in the entry's `review.json`). Source tokens are
measured, reported and minimized, but a twin above the original's count still
passes: a longer twin points to a missing Core API or syntax, which its entry
names in `tool.token_gaps`, and clarity of reasoning outranks terseness. Then
the repository switches to the Jet twin and keeps the original only as the
entry's reference in the entry's directory. `Tools/Gauntlet/PortBars.jet`
judges every tool entry on all four bars. `Tools/Agent/ScriptPairing.jet
--check`, which every lane check and every commit's ratchets run, refuses a
change that adds an in-scope script in another language without its pair, or
that switches an in-scope entry before it passes every bar; Nix files are not
scripts. A library counts as covered once every in-scope tool script that
imports it, directly or through other importers, is paired or covered (its
job is part of each importer's, so it is replaced only when all of them are);
a `*.test.mjs` counts once its imports are all paired or covered. `--census`
lists every script in another language in the checkout as paired, covered,
unpaired (with the importers that keep it so) or out of scope.

## Greenfield cutover

Jet owes no compatibility to its own history until the owner declares
otherwise. Ship one canonical current form. When syntax, semantics, an API, an
ABI, a format, a name, a path, or a command changes, migrate every in-repository
caller, example, test, generated artifact, schema, and affected explanation in
one cutover. Remove old spellings, aliases, shims, fallback parsers, legacy
readers, version branches, and parallel implementations. Ratification history
belongs in Tower and necessary design rationale in specs, never as retired
behavior in the compiler or tools. A compatibility exception needs an
owner-ratified decision naming its exact scope and removal condition.

## Invariants

A violation stops the affected work until it is fixed at the root.

- **I1 — Safety.** Jet is memory-safe and type-safe by default. Memory safety
  includes leak freedom: safe Jet cannot leak memory, and only code inside an
  `#Unsafe` block or function may. The expert escape hatch is a user-written,
  audited `#Unsafe("reason") { … }` block or `#Unsafe("reason") fn`. Generated
  `unsafe` operations appear only there or in vetted standard-library and
  memory internals.
- **I2 — Hidden backend.** If the native backend rejects or cannot lower checked
  code, that is an internal compiler error with exit code 101, never a user
  diagnostic.
- **I3 — Sema owns checks.** All language checking lives in sema. Codegen lowers
  facts sema already established; it never probes the backend to find user errors.
- **I4 — Complete diagnostics.** Every diagnostic has a registered code,
  what/why/fix text, and a UI snapshot. Without the snapshot it is not done.
- **I5 — Executable examples.** Every feature has an example with golden-tested
  output, and the example proves the same meaning on every applicable tier.
- **I6 — Dependency seams.** The compiler and its seam packages use path
  dependencies only. The ratified stdlib bootstrap dependencies are temporary.
  Any new stdlib external dependency needs an owner decision and an approved
  bridge pattern.
- **I7 — Ratified syntax.** Every keyword or sigil a user can type is registered
  in the syntax registry (`Compiler/JetFoundation/Data/Syntax`) and ratified on
  Tower. Decision IDs and decision history live in Tower and the specs, never
  in source: no decision fields in registries, no decision citations in code
  or comments.
- **I8 — One mechanism.** Each meaning has one canonical semantic mechanism.
  Spelling or organization may flex only where that helps users. Keep the
  beginner surface small and safe, and expose expert control only by explicit
  opt-in. A new mechanism needs approved Tower scope or owner approval.
- **I9 — One meaning across tiers.** Every language feature and Core API has
  one executable meaning on every tier: the Jet code generator's levels (O0
  for `jet run`, `jet dev`, and compile-time code; O1 for `-O1`;
  O2 for `jet build`) and web where applicable. That meaning lives in the embedded
  Prelude and the ratified CoreLib, compiled once into the one shared runtime
  (D-EXEC1). Code generators and hosts only marshal values and call the same
  runtime symbols; they never re-encode validation, defaults, policy, or error
  meaning. Never mark a feature as working on one level only, or defer it to a
  later level. `jet run` and `jet dev` compile at O0 and run the linked image
  from memory without writing an executable (D-EXEC2=A); compile-time code
  also runs natively at O0, in a child of the compiler, with the same meaning.
  No interpreter or second code generator serves any tier. An owner-ratified
  exception must name the tier it excludes.

## Performance gate

Jet's performance target has two stages. The first stage is the peer floor,
and it is a hard, structural requirement: a Jet program must be faster and use
less peak memory than the same program in every other language. Jet/peer must
be below `1.00` for every matched peer, for each required cell and for both
wall time and peak memory. Rust is judged like every other peer, with no
parity band. A loss to any peer on either metric is a codegen, runtime or Core
failure. It is fixed generally and structurally, so that any Jet program beats
the other languages, never only the measured one. The second and final stage
is the machine ceiling: hand-written, maximally optimized machine code for the
same workload, which Jet must match or beat in both cycles and bytes
(Jet/ceiling at most `1.00`). Jet aims as close to the ceiling as it can.
Closing the remaining ceiling gap is the hill-climbing stage: until the owner
opens it, a cell passes the gate on the peer floor, and its ceiling ratio is
measured and reported but does not fail the gate.

Measure live. Jet's goal is raw compile and run speed, not incremental or
cached compilation: the compile-speed target is 30 million lines per second
from cold. A measured run compiles from cold, and its compile time counts. No
script cache, build-action reuse or other compile cache serves it, and every
compile cache is off by default. Computers are fast and software is bloated,
which is why the final comparison is always hand-optimized machine code.

Every required cell needs a ceiling reference: an asm-parity cell
(Tools/Perf/AsmParity) that names its gauntlet cell and does the work with the
best known technique for the workload (SIMD, algorithmic, or syscall-level),
not a naive transliteration. A cell without a reference is uncovered and fails
the gate once hill climbing opens the ceiling stage. A better technique
replaces the reference and raises the target.

The ceiling delivers the same features as the Jet program. Where a Jet feature
(diagnostics, safety checks, fault reports) physically costs bytes or cycles,
the reference includes the smallest hand-written implementation of that
feature. Jet minimizes each feature's cost and never removes a feature to meet
the ceiling.

Close every performance gap at its root, in the general mechanism: the
codegen, runtime, or Core path that every program using the feature goes
through. Never tailor a fix to one cell: no pattern that recognizes a
benchmark program, no per-cell flag or constant, and no special case that only
that workload reaches. When the ceiling shows that a feature costs almost
nothing (for example, CPU time while waiting on I/O), Jet's general code for
that feature must cost almost nothing too. Every performance fix names the
feature it improves and proves the gain on a second program that uses the
feature differently from the cell.

Required cells cover the foundations (numerics, text, files, concurrency,
networking, build time, and run time) plus one real workload in each critical
area: web, games, CLI and scripts, data analysis, backend services, GUI
applications, and embedded. A niche becomes required only when
Jet ships a first-party battery for it; never claim a niche win without one.

Apply the comparator separately to each cell and metric. Never average away a
loss, substitute an easier workload or tier, drop a peer, or accept wrong,
unavailable, uncovered, mismatched, or inconclusive evidence. A performance
card, milestone, dashboard, or release gate stays open while any required cell
fails. Never trade semantics, diagnostics, determinism, safety, or I9 parity
for a score. Manifests and gates must encode this policy; prose alone is not
evidence. Historical receipts stay immutable under the policy they were
recorded with and never weaken the current gate.

A surface added for performance must arrive with a paired two-program cell
comparing it with the plain spelling it replaces. Ratification waits until the
surface strictly beats the plain form. Record the pair in the canonical
manifest and keep any loss on a card.

## Biggest wins first

This is a standing owner order, repeated many times, and it binds every plan,
dispatch and landing inside an epoch. Work on an epoch is done in order of its
expected gain on the epoch's metric, largest first, and the largest steps start
first even when they are architectural, long or hard. It ranks the work inside
an epoch; it never reorders the epochs, which only the owner orders (see Owner
gates and work state).

- **Rank before work.** Before any agent starts on an epoch, its plan on the card
  lists every candidate step with its estimated share of the gap, measured from
  a profile wherever one can be taken (for compile speed: CPU microseconds per
  line and wall seconds), sorted largest first. A step whose combined ceiling
  cannot reach the epoch's target is named as such; it never stands in for one
  that can.
- **Staff the top.** The steps that carry most of the gap get the agents first.
  An architectural step is never deferred behind tactical work because it is
  larger, slower to land or riskier. Tail work (any item worth less than a
  tenth of the largest open item) waits until every larger item is landed or in
  flight, unless it unblocks one.
- **Every submission states its gain.** Each submission names the metric it
  moves and its expected gain. The orchestrator ranks the queue by that gain,
  refuses a submission that states none, and reports the measured gain against
  the estimate at every check-in.
- **Re-rank on evidence.** When a measurement shows the ranking was wrong, the
  plan is re-ranked and the agents are moved the same hour.

A plan that is not ranked, or a dispatch that puts a smaller item ahead of a
larger one, is an orchestrator failure and is fixed at once.

## Owner gates and work state

Before coding, identify the choices only the owner can make: new syntax, public
API, commands, dependencies, invariant or I9 exceptions, epoch or scope moves,
product behavior, and visual direction. Put each one on a Tower ballot with
same-program alternatives, exact syntax and behavior, trade-offs, edge cases,
and the beginner and expert paths. Do not ballot implementation choices an
approved contract already covers. A question is not approval, and a ratified
outcome stays in force until the owner changes it.

This is a standing owner order, repeated hundreds of times: no agent asks the
owner questions. No questions in chat, none as card logs, notes or "owner
question" entries, and none that hold work. Within approved scope, the agent
decides, records the decision as fact on the card, and keeps working. An
owner-only choice goes on a ballot and the work moves to independent criteria
meanwhile. Workers send their questions to the orchestrator, and it answers them
the same way.

The orchestrator works the open epochs in the owner's epoch order, which Tower
records as each epoch's `order`, until every criterion is met. This is a
standing owner order: three of the four agents always work the first unfinished
epoch in that order and the fourth works the epoch after it, and when an epoch
is done the staffing moves one epoch along. Work the owner puts ahead of the
epoch order takes the agents the owner gives it until it is done. Implementers
write the code, land it on LaneCheck and report the commit; the orchestrator
tests it in batches and sends every failure back to its implementer to fix.

Tower is the only work ledger. Every incomplete stream has exactly one card
recording its phase, plan, dependencies, criteria, and handoff state. Workers
never write to Tower, close cards, or keep a competing task list. The
orchestrator integrates a valid result, runs the exact focused proof the
criteria name, records the evidence, closes the card, and confirms it is `done`
before claiming or briefing more work. A milestone then gets one commit-bound
composed sweep and one fresh-context review; individual cards do not wait for
that gate.

A new card is earned only by a new stream of work: a ballot, a feature, or an
epoch's plan. A regression, follow-up, or gap is a criterion on the card whose
work caused it or already owns the area, never a card of its own; findings of
one sweep share one card. This is a standing owner order (2026-11-14).

One implementer owns each coherent patch. Concurrent writers need disjoint
paths and one named close owner. The repository has at most three branches:
`master`, `dev`, and one other. Today the one other is `rust`: at the Rust
cutover, the last commit that still contained the Rust implementation became
the `rust` branch, a frozen dead-end leaf kept as a reference and a
re-bootstrap safety net. Nobody commits to it or merges it into `dev` or
`master`. While `rust` exists, no other branch exists and every change lands
directly on `dev`; if the owner retires `rust`, the one other branch may be a
single `staging` branch whose commits merge into `dev`. This is a standing
owner order: `dev` holds all the work and is the staging ground where every
change is proven first; `master` holds the board plus all the code of the
latest successful compiler. Each batch compiler that builds from the `dev` tip
and passes its self-check is merged into `master` at once (the merge takes
`dev`'s code and `master`'s board), so no proven work lives only on `dev` and
no work at all lives only in a worktree. Both branches are pushed after every
commit, board commits included, so `origin` never lags. Only the orchestrator,
or the one Lander it names (see Delegation), commits, directly on `dev` in the
dev worktree, one commit per landed submission that stages only that
submission's paths, and merges proven `dev` commits into `master`.
Every worktree has one named, live owner: the dev worktree (the orchestrator),
the Lander's trees, and at most one per standing agent within the team cap.
This is a standing owner order: besides the master checkout, at most six
worktrees exist at any time, and no work in them is ever lost. Ten worktrees,
the master checkout included, is a hard cap that the repository's git hooks
enforce: `post-checkout` removes an eleventh tree as it is added, and
`reference-transaction` refuses every ref update while more than ten exist.
Only the owner's explicit written permission raises the cap. An agent never
opens a second worktree; it moves its one worktree forward to the `dev` tip
for each new task.
Each one except the dev worktree is a detached checkout of `dev`, kept at a
current `dev` commit (moved forward whenever its owner's work lands, never a
day behind), and holds no build output and no commits of its own. It is
closed as soon as its owner is done with its work: the work is integrated
into `dev` (and its board state into `master`) or recorded on its card first,
then the worktree is removed. Stale, orphaned and superseded worktrees are
removed the same way whenever found and at every milestone. The master
checkout tracks `master`: its board files are the live board, and its code is
the latest successful compiler's, refreshed by each merge from `dev`. Nothing
but the board and those merges is ever committed there, and work reads and
runs code from the dev worktree. Never create
any other branch, worktree, or other
ref (`refs/keep`, `refs/salvage`, `refs/archive`, private lineages), for any
purpose: experiments, candidates, measurements, and proofs included. Work
happens in these worktrees; no agent keeps its own copy of the repository. A
`git archive` export
outside the repository is allowed only for one build or measurement that can
use neither the worktree nor a published compiler (a release gate, or an A/B
base whose compiler is not published), and the job that made it deletes it
when it ends. Use checkout-owned external targets (see the orchestration
resource reference). Never overwrite another
task's paths. Never run `git add -A`, a broad `git commit -a`, `git stash`,
`git restore .`, `git reset --hard`, or any equivalent broad operation. Never
hand-edit `Tools/Tower/.tower/`.

Scratch feeds work into `dev`; it is not an archive. Before removing anything,
save the work it holds: commit it on `dev`, or record on its Tower card exactly
what remains unlanded and why. Once the work is on `dev`, delete everything
that fed it: exports, build trees, staged patches, test scratch and logs. Each
agent deletes its own scratch when its commit lands, and keeps at most one
worktree at a time.
Keep only the compilers in use: the admitted seed, a candidate under test, and
the compilers a running job names. Never retire files by moving them elsewhere
on disk; git history and Tower are the record. Audits and final reports under
`Docs/` stay owner-only deletions (see Documentation).

Only the owner starts `tower serve` unless the owner directs otherwise. Agents
use the non-serve Tower CLI (`node Tools/Tower/tower.mjs`) against the main
board and report any stale or duplicate server. Never touch the owner's
personal notes or scratch files.

Committed files carry no personal or machine information: no home paths, user
or host names, personal emails or machine mounts. Write a placeholder instead,
such as `$HOME`, `$USER`, `$JET_SCRATCH_ROOT` or `$JET_STATE_ROOT`; fixtures
use the example account `user` on `devhost`. This holds for recorded evidence
too. `Tools/Agent/LaneCheck.jet` refuses a checkout that breaks it, through
its personal-information guard (`--personal-info`, #6564 c5).

## Adversarial design review

This is a standing owner order. No design may be built, shipped, or described
as better than existing solutions until an adversarial review has tried to
break each of its claimed strengths. The review is a dated audit in
`Docs/Audits`. For every claim it:

- names the strongest existing solutions and states their best case fairly,
  with sources, including what they do that the design does not;
- attacks the claim: hidden costs, cheating (an easier workload, a narrower
  scope, a dropped feature, a deferred hard case), unhandled edge cases,
  failure modes, tooling, migration and ecosystem gaps, and what the design
  gives up;
- checks the claim against code, a runnable probe, or a measurement wherever
  one is possible, never prose alone;
- ends with a verdict: holds, holds only within stated limits, or fails, and
  puts every gap on a card or ballot.

Missing context is a failed claim, not a pass. A design with a failed claim or
an open gap stays unbuilt until it is redesigned and reviewed again. As with
ballots, every downside the review finds is engineered away, and the redesign
is reviewed again, before the design is presented to the owner as the
solution; a downside that cannot be removed is named with its cost and its
best mitigation. A ratified ballot authorizes the direction, not the claim;
its build card names the passing review as its first criterion. Public text
may call a design better only where a passing review backs that exact claim.

This is a standing owner order. A design is the whole mechanism, never a fix
for the one case that raised it. Before a ballot is drafted, its author finds
every place the same kind of fact or operation already lives (each record, table,
maker, reader, tier and tool) and every similar construct that should share
the mechanism. The design makes one source of truth for all of them, with an
exact cutover list and the copies it deletes. A ballot that leaves a parallel
store, a second maker or an untreated sibling case fails its review.

## Delegation

Use the most specific agent available for each role. Workers do not spawn
workers.

This is a standing owner order. Every agent does its work on Claude Opus, and
only the owner chooses another model for any role. No review needs a reviewer
from a different model family.

Workers are never idle or stalled. Each worker runs one continuous loop: write
the code to the card's spec, run only the touched files' source checks,
submit the patch to the orchestrator's inbox, and start its next task at once.
Workers never commit, build the compiler, or run suites, A/B comparisons,
chains, censuses, or releases. When a spec or criterion exists, the code comes
first: no worker stops to prove rows or waits for review or a test result.
While the orchestrator's batches run, every worker keeps coding to spec.

A worker's unit of work is one open criterion, read from the card before any
code is written. If its proof already passes, the worker sends the command and
its output and takes the next criterion. Every item is sized at most one hour
of code plus one hour of testing and fixes. Larger work is split into such
items and written, never sized in hours or days, parked or noted as blocked
for its size. The only blockers are an open owner ballot and a missing
dependency another lane owns; the worker names it to the orchestrator in one
line and moves on. A worker never ends its turn to wait on its own job.

This is a hard owner requirement. The orchestrator never lets a worker idle.
No worker waits on a landing, a batch, a seed, a review, or a proof: it stacks
its next submission on its own unlanded ones and keeps writing code to spec.
Workers deliver large batches of work at once, never one small slice per test
cycle. When failures come back, the worker fixes the whole list in parallel
with its new work and resubmits it in one pass. A worker that is waiting is an
orchestrator failure. Fix it at once by queueing that worker's next task or
reassigning it.

The orchestrator does every build and test. It keeps every worker's next task
queued and lands each passing submission on `dev` as its own commit. Landing
runs one continuous cycle, never a train, and the heavy-job slot never sits
idle while a submission waits (owner order, 2026-11-15):

1. **One census takes everything ready.** A cycle starts with every
   submission in the inbox and checks them together in one collect-all census
   (`Tools/Agent/Land.jet census`). A submission that conflicts only in a
   generated file is applied without that file, and the generators are rerun
   (`Tools/Agent/DiagnosticRows.jet --write` and every generator the census
   checks). Only a conflict in hand-written source goes back to its worker. A
   failing census row holds back only the submissions that cause it; the rest
   land on `dev` at once, one commit each.
2. **The census build is the batch compiler.** The census's own
   `jet build Compiler` becomes the compiler every intake uses; nothing is
   built twice. When the census held a submission back, its snapshot is not
   the landed tip, and the batch compiler is built from the tip instead. If a
   build breaks, the commit that broke it is reverted at once, so `dev` always
   builds. Each batch compiler is a bisect point.
3. **Every batch compiler runs the full suite.** On it, at once: each landed
   submission's proof lines (from its `tests` file) and the whole suite in
   collect-all mode (the cases profile beside ui, golden and core, and the
   unit tests of every compiler package and its `Tests` package), compared
   with the previous cycle's run. A new failure can only come from that
   cycle's submissions, so it goes to its worker at once, as one list, and
   becomes that worker's top priority. Passing proof lines meet their criteria
   on Tower in the same cycle.
4. **The next cycle starts at once.** Submissions that arrived during a cycle
   start the next one as soon as the slot frees. About every two hours, the
   speed gate and self-check run on the current batch compiler, inside the
   cycle. Each full suite run with no new failures may become the owner's next
   a0.0.N release through `Tools/Release/Main.jet` `admit`.

Workers check against the current batch compiler (the seed) before they
submit, so a cycle finds only what their touched-file checks cannot. Every patch
is built on the `dev` tip; a Core half proves on the seed that it compiles and
lowers; the lander runs the guards and ratchets before each build; and each
landed batch's proven criteria are met within the hour
(`.agents/skills/orchestration/references/intake.md`, train rules 21 to 24).

Landing throughput is the measure of progress. These standing owner orders
keep it high:
- A submission meets whole criteria and names each one with its exact proof
  command. Light work travels in its own submission, apart from compiler and
  Core changes, so it is never held by a build.
- Each lead reports every hour: the submissions it staged and the criterion
  each one meets.
- Criteria that already hold on `dev` are found by sweep, proved and met
  directly, with no new code.

The orchestrator may name one Lander and one Closer. The Lander runs the
landing cycle in the orchestrator's place, landing whatever passed and
sending every failure to its owner. The Closer
takes the cards with the fewest open criteria, runs each criterion's proof,
and hands the orchestrator the evidence. Tower writes, owner gates and
dispatch stay with the orchestrator.

The orchestrator evaluates its own plan continuously, not only when something
fails. Before each step and whenever a result arrives, it asks whether this is
the fastest effective route to the owner's targets, whether a better approach
exists, and whether work, order, or agents should be rearranged to meet every
deadline. Then it acts on the answer: it re-routes work to whoever can finish
it soonest, puts the highest-value and quickest wins first so progress shows
early, and overlaps every independent stage. The orchestrator orchestrates; it
takes on implementation only when that is the fastest route and never at the
cost of monitoring, unblocking, and verifying.

This repository carries no harness-specific configuration. Agent tools use
their own global settings; Jet contributes only this contract and the skills
under `.agents/skills` and `Tools/Tower/skills`.

## Find every issue at once

This is a standing owner order, repeated many times; violating it wastes hours.
It binds every agent on every step of every effort, whether or not the owner
repeats it, and nothing overrides it short of an explicit owner instruction to
rerun one thing. Never discover problems serially, where one blocker is found,
fixed and rerun to find the next. Before fixing anything, collect every issue
in one pass:

- **Expensive launches are gated, never bypassed.** A stage0, self-hosting
  chain, release build or any other run costing more than about 15 minutes
  starts only after a clean collect-all preflight of that exact commit:
  `jet Tools/Agent/Land.jet census` (the seed's keep-going `jet check` and
  `jet build` of the compiler, the runtime pack and the self-checks of the
  CLI and of every Core body, every row of every phase in one report; the
  lander runs it through `Land.jet
  precommit` before every compiler commit) and `jet Tools/Agent/Chain.jet
  TREE LEVEL --dry-run`, which names every stage the launch would build. Every
  row is clean or carries a written disposition naming the commit in this
  launch that fixes it. Launching to "see what fails next" is forbidden. The
  launchers enforce it: a clean census writes the snapshot's receipt
  (`Land.jet receipt`, keyed on the compiler source identity and the seed),
  and `Tools/Agent/Chain.jet TREE full` and `Tools/Release/Main.jet` `admit`
  and `build` refuse to start without the receipt for their tree and seed;
  `--check-gate` gives the answer without launching.
- **Every compiler phase has a collect-all mode, or that is the first defect.**
  Parse, imports, sema, comptime, codegen lowering, backend lowering, machine
  emission, object, link and run must each record a failing item as a census
  row, trap or skip it, and continue. A process-stopping `panic` that guards
  an assumption about checked input is a collect-all defect: convert it to a
  keep-going report before relying on that phase to gate anything.
- **Use waiting time to run later stages early.** While an expensive build
  runs, probe every later stage with the best available artifact (the last
  good factory, an older compiler, a dry run) and fix what they find in the
  same batch.
- **Run every stage in collect-all mode.** Use the existing keep-going
  mechanisms (`--keep-going` all-phases reports,
  `JET_DEBUG_LOWER_ALL=1` lowering sweeps, per-error census scripts) so one run
  reports all errors in all items, not the first one. A tool that stops at its
  first failure needs a collect-all mode before it gates anything.
- **Preflight the whole pipeline end to end first.** Before starting a
  multi-stage effort (self-hosting, a cutover, a release candidate), dry-run or
  probe every stage up front: parse, imports, sema, lowering, emission,
  generated tables, runtime pack, link, run, tests. That maps every blocker,
  dependency and missing piece before the first fix.
- **Partition when a run is slow or hangs.** Split the input and check the parts
  in parallel to localize every failure at once.
- **Fix the whole list in parallel.** Group the findings by root, give each root
  one owner, and land them as one batch before the next expensive build or
  check. A rebuild that exists only to reveal the next error is a process
  failure.
- **A tightened rule sweeps everything first.** A change that makes sema,
  a guard or a gate stricter is checked, before it lands, against every Jet
  source the rule can reach: Core, Compiler, Jetpack, Tools, Examples and every
  Tests/ tree, case fixture and template. Every hit is fixed in the same
  submission or in one it stacks on, never in a later cycle: the census builds
  with a seed that lacks the new rule, so it cannot see the rule's hits.
  Checking only what the compiler build or one suite touches is a serial
  discovery.
- **A failed gate is read whole.** When a release, chain or suite fails, read
  every lost row of every suite, sweep for the same root across the whole
  repository, and fix the full list as one batch before the next run.
- **Gate every commit cheaply first.** Parse and check each touched file,
  regenerate and commit generated outputs with their sources, and run the
  generator checks against the exact source a build will use, so no expensive
  run is lost to a mistake a fast check would catch.
- **Run independent stages in parallel.** A pipeline's dependency order does not
  make its probes serial. Start every stage that can run now (a host build
  beside a sweep, checks of later-stage inputs beside earlier stages), and
  make each stage keep going past its failures far enough that the next stage's
  own collect-all census runs in the same cycle (for example, write partial
  output so a downstream type check reports everything else at once).
- **A stage only stops at a failure that blocks every later item.** Everywhere
  else it records the failure, skips the item and continues, then reports the
  full list and returns the first error.
- **Never claim a job started until it is observed running.** After releasing
  queued work, confirm it in the scheduler's actual running state (the real
  queue path, its start decision, its live process) before reporting it as
  started; a misplaced job file is a silent day-long stall.

## Environment and proof

Run repository commands with `jet` directly; `JET` names the admitted
native compiler (the stage-2 output of `jet build Compiler`) for
every tool and gate. Keep scratch and logs on disk and build output outside the
repository. Follow the orchestration resource reference for bounded,
checkout-owned targets and shared artifacts.
Rebuild with `jet build --profile dev Compiler` (the fast build) before compiler
smoke tests. After integration, run the exact narrow proof the criteria name.
Outside the landing cycle's full suite on each batch compiler (Delegation),
broad suites, unfiltered censuses, and `Tools/Agent/VerifyFull.jet` are
milestone or release operations and need the required token. A worker's
type-check is not proof of runtime behavior, tier
parity, goldens, snapshots, or generated artifacts.

**The owner's machine stays responsive.** This is a standing owner order:
agent work froze the owner's desktop on 2026-11-11 and forced a power-off.
The shared scratch disk is a slow drive, and a backlog of unwritten data on it
stalls every program on the machine. So:

- All agent work runs in the bounded agent slice, the same one the machine's
  guard freezes when unwritten data or free memory threaten the desktop. Every
  launched job (build, test, check or measurement) runs inside it; nothing runs
  in the owner's session.
- At most one heavy job runs on the machine at a time: a compiler build, a full
  or sharded suite, a release admit or a timed measurement build. Only the
  orchestrator or its Lander starts one. Workers run only touched-file checks
  and single-file tests, one at a time.
- No job traces a whole build (`strace -f`, `perf record -a` and the like), and
  no tool reruns a check in a loop. A measurement takes its own turn in the
  heavy-job slot; it never runs beside a build.
- Before a heavy job starts, its launcher checks the machine: memory available,
  unwritten data on disk and scratch disk use. It waits, and never starts, while
  the guard holds the agent slice frozen or the scratch disk is over 75% full.
- **The scratch disk never passes 75% full.** This is a hard owner limit, not a
  cleanup prompt: past it the slow drive slows the whole machine.
  `Tools/Agent/Chain.jet slot` opens no build slot above it. The orchestrator
  watches the disk at every check-in. When it passes 75%, freeing space comes
  before all other work, and the orchestrator reports the cause to the owner.
  Free space by deleting landed or superseded build output first, then
  worktrees whose work is saved, never by losing unlanded work.

Agent tooling lives in the repository: `Tools/Agent/Chain.jet` (the self-host
chain and the machine build slots), `Tools/Agent/Land.jet` (`serve` is the
lander, `batch` its batch landing, `census` the preflight, `publish` keeps
landed compilers for lanes; `queue` adds a land job for the lander; `land`
lands a patch stack directly, with `--scratch-copy` on a fresh clone),
`Tools/Agent/Suite.jet` (one `jet test
Compiler --profile` run on a compiler snapshot) and `Tools/Agent/Ballots/Validate.jet` (the ballot validator).
Tools find state, scratch, queues, published compilers and seeds only through
the root variables of `Tools/Agent/Orchestrator/Roots.jet` (`jet
Tools/Agent/Orchestrator/Roots.jet NAME...` prints them; `Tools/Agent/roots.sh`
and `Tools/Agent/roots.mjs` keep the same table for the shell and node tools
not yet ported to Jet). Their defaults derive from `$HOME` and the XDG directories; a machine
sets its own values in the environment or in
`${XDG_CONFIG_HOME:-~/.config}/jet/env`. No
committed file spells one machine's home, user, mounts or tool roots:
`Tools/Agent/LaneCheck.jet`'s personal-information guard, which every lane
check runs first, refuses them, and
`tower redact` cleans the board.

There is no hosted CI until a pinned native seed release exists
(D-SEED-DIST1). `Tools/Agent/LaneCheck.jet`, the Core surface ledger check
(`jet Tools/Agent/CoreSurfaceLedger.jet --check`) and the full gate
(`JET=<jet> jet Tools/Agent/VerifyFull.jet`, which includes the Core
source-coverage ratchet `Tools/Agent/CoreCoverage.jet`) run locally with the
admitted `JET`.

## Dogfood the debugger and profiler

This is a standing owner order. Every agent debugs a Jet program with `jet
debug` and profiles one with `jet perf`, and that includes the compiler and
every tool, which are Jet programs too. Jet controls the compiler, the runtime
and the image, so its own tools must answer every question an external one
would; using them is how their defects are found.

- **No external debugger or profiler on Jet code.** gdb, lldb, Linux `perf`,
  valgrind, rr, strace and their like run on Jet programs in exactly two
  places: as the named baseline of a comparison harness (such as
  `Tools/Perf/DebugLatency`, which times lldb), and for a question the Jet tool
  cannot answer yet. In the second case the gap goes on the dogfooding card as
  a criterion first, with the command that failed, and the note names it; the
  external tool answers that one question only.
- **Real work runs through them.** Compile-speed ranking takes its profile from
  `jet perf` on the compiler. A codegen or speed-gate regression is profiled
  with `jet perf` before it goes back to its author. A crash, hang or wrong
  value in a Jet program, the compiler included, is diagnosed with `jet debug`
  (live, `--attach`, or a `--record`ed replay) before anyone reads code to
  guess.
- **Every friction is a defect.** A missing command, a wrong or missing value,
  a slow step, a crash or a confusing output in `jet debug` or `jet perf` goes
  on the dogfooding card as a criterion with its reproduction, the same hour.
  Fixing those is ranked like any other defect.

The `diagnosing-bugs` skill shows which command answers which question.

## References

Read this file first, then load only what the task needs:

| When the task involves | Read |
|---|---|
| Dispatch, waves, worktrees, receipts, recovery, or card closure | `.agents/skills/orchestration/SKILL.md` |
| Compiler or language semantics | The relevant code, examples, and tests; `Docs/Spec/syntax-decisions.md` for decision rationale |
| Diagnostics or snapshots | `Compiler/Prelude/Diagnostics.jet`, the matching UI snapshots, and `Docs/Spec/diagnostics.md` |
| Examples or golden paths | `Docs/Spec/Contributing/examples.md` and `Examples/README.md` |
| Tower cards, questions, tags, or board operations | `Docs/Spec/Contributing/issue-tracker.md`, `Docs/Spec/Contributing/triage-labels.md`, and `Tools/Tower/skills/tower/SKILL.md` |
| Domain vocabulary or decisions | `Docs/Spec/vocabulary.md`, `Docs/Spec/syntax-decisions.md`, and `Docs/Spec/Contributing/domain.md` |
| Security closure | `Docs/Spec/Contributing/security-closure.md` |
| Technical traps | `Docs/Spec/Contributing/agent-engineering.md` |
| Audits, research, cleanup, or skill routing | `.agents/skills/JetSkillsRouter.md` |
| Completion claims or milestone closeout | `.agents/skills/verify/SKILL.md` |

For coding and technical design, understand the whole path first, then choose
the smallest complete change. Write durable artifacts in clear, plain prose.
When a task asks for implementation, do not stop at a report, and never claim
work that the board or an exercised command does not prove.
