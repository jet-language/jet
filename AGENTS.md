# AGENTS.md — Jet agent contract

This is Jet's baseline contract for every agent and tool. Vision and ranked
priorities live in `Docs/Spec/philosophy.md`. Plans and work state live only in
Tower. Multi-agent delivery and task procedures live in skills.

## Authority and verification

Code and executable registries are the source of truth for what Jet currently
does; specs explain durable intended contracts. When code conflicts with a
ratified requirement, treat it as a defect in Tower—never bless stale behavior.

Precedence when rules conflict (newer ruling from higher authority wins; never
average conflicting rules):

1. The owner's latest explicit instruction.
2. A ratified Tower decision and its acceptance terms.
3. The relevant domain specification or ADR.
4. This contract.
5. The task-specific skill.

**Verify before claiming, carding, balloting, or editing:**

1. **Check syntax in the registry first.** Read
   `Compiler/JetFoundation/Data/Syntax/Spellings.jet` (and siblings) before
   naming any keyword, sigil, or grammar form. Never invent syntax from other
   languages.
2. **Read spec and live code together.** Check `Docs/Spec/spec.md`, the relevant
   `Docs/Spec/Reference/` page, and the implementing compiler/Core source before
   claiming a feature is missing or broken.
3. **Check Tower first.** Search existing cards and decisions before filing new
   ones; never duplicate a card or re-ballot a settled choice.
4. **Cite exact sources.** Every claim about what Jet allows, forbids, or ships
   must rest on a file, symbol, registry row, or command output read in the
   current session.

## Collaboration and tone

Act as a skeptical peer engineer, never a yes-man.

- **No flattery or cheerleading:** Skip praise, exclamation-mark hype, and
  reflexive validation. Respond plainly and directly.
- **Challenge gaps and weak assumptions:** Push back when an idea has
  ambiguities, second-order costs, edge cases, or conflicts with Jet's
  invariants—especially where the owner may lack compiler or PL context.
- **Refine rough ideas instead of dismissing them:** When an owner's rough idea
  contains a genuine insight, steel-man the core intent, name the exact flaws
  and trade-offs honestly, and engineer concrete alternatives so the owner can
  decide on substance.

## Biggest wins first (Pareto gains)

Unless the owner explicitly directs specific work, **always work the largest
Pareto gains first**—even when they are architectural, larger, or harder.

- **Rank before starting:** List candidate steps by measured or estimated share
  of the gap (from a `jet perf` profile wherever possible), largest first.
- **Staff the top, not the tail:** Do not deliver tail optimizations (items
  worth less than a tenth of the largest open item) while major Pareto wins
  remain open, unless they directly unblock one.
- **Re-rank on evidence:** When a measurement disproves an estimate, re-rank
  immediately and move work to the real bottleneck.

## Repository layout and hygiene

| Path | Holds |
|---|---|
| `Compiler/` | Jet compiler, native backend, runtime, embedded Prelude (`Compiler/Prelude/`), bootstrap |
| `Jetpack/` | Jet package manager |
| `Core/` | Core library sources |
| `Examples/` | Executable examples and golden outputs |
| `Tests/` | Native test corpora, UI snapshots, fixtures, compiler guards |
| `Docs/` | `Spec/`, `Audits/`, `Research/`, `Proposals/`, generated `Site/`, `Assets/` |
| `Tools/` | Contributor tooling (`Agent/`, `CI/`, `Perf/`, `Gauntlet/`, `Editors/`, `Tower/`, `Release/`) |

- **Naming:** Folders are TitleCase with ALL-CAPS acronyms (`Tests/`,
  `Compiler/JetCLI/`). Keep the top level small.
- **Docs discipline:** `Docs/Spec/philosophy.md` holds vision only (no plans or
  status). Never commit generated catalogs or duplicate status tables in docs.
  Audits and research keep dated findings, never live work state; only the owner
  may delete audits or final reports.
- **Greenfield cutover:** Ship one canonical current form. When syntax, APIs,
  ABIs, formats, paths, or commands change, migrate all callers, tests, examples,
  tools, and docs together and delete legacy shims/aliases unless an
  owner-ratified decision explicitly keeps them.
- **Repo tooling is Jet:** Scripts in `Tools/Agent/`, `Tools/Release/`, and
  `Tools/CI/` are written in Jet—or paired with a Jet twin under
  `Tools/Gauntlet/Entries/` until the twin passes `Tools/Agent/ScriptPairing.jet`
  (speed, peak memory, reasoning bar). Non-repo tools (editor extensions, perf
  harnesses, peers) stay in their host language.
- **No personal/machine data:** Never commit build outputs, scratch files, home
  paths, usernames, hostnames, emails, or machine mounts—including in recorded
  evidence. Use placeholders (`$HOME`, `$USER`, `user@devhost`).
- **No harness config:** This repository carries no harness-specific
  configuration. Agent tools use their own global settings.

## Invariants

A violation stops the affected work until fixed at the root.

- **I1 — Safety.** Safe Jet is memory-safe, type-safe, and leak-free by default.
  Unsafe operations appear only in user-written `#Unsafe("reason") { … }` /
  `#Unsafe("reason") fn` or vetted stdlib/runtime internals.
- **I2 — Hidden backend.** If the native backend rejects or cannot lower checked
  code, that is an internal compiler error (exit code 101), never a user error.
- **I3 — Sema owns checks.** All language checking lives in sema; codegen lowers
  established facts and never probes the backend to find user errors.
- **I4 — Complete diagnostics.** Every diagnostic has a registered code,
  what/why/fix text, and a UI snapshot. Never reuse a code registered for a
  different meaning, and keep registry text in step with the rule actually
  shipped.
- **I5 — Executable examples.** Every feature has a golden-tested example
  proving the same meaning on every applicable tier. Its golden must show the
  feature's own output (a report, diagnostic, or footprint), never only "ok".
- **I6 — Dependency seams.** Compiler and seam packages use path dependencies
  only. New external stdlib dependencies require an owner decision.
- **I7 — Ratified syntax.** Every user-typable keyword or sigil is registered in
  `Compiler/JetFoundation/Data/Syntax/` and ratified on Tower. Never put decision
  IDs or ratification history in source, registries, or code comments.
- **I8 — One mechanism.** One canonical semantic mechanism per meaning; keep the
  beginner surface small and safe, with expert controls explicit and opt-in.
- **I9 — One meaning across tiers.** Every language feature and Core API has one
  executable meaning across O0 (`jet run`, `jet dev`, comptime), O1, O2
  (`jet build`), and web where applicable—implemented once in the shared
  Prelude/CoreLib runtime. No interpreter or second code generator; never mark a
  feature working on only one tier without an owner-ratified exception.

## Performance standard

- **Peer floor (hard gate):** For every required cell and both wall time and peak
  memory, `Jet/peer < 1.00` against every matched peer (including Rust, with no
  parity band).
- **Machine ceiling (hill-climbing target):** Match or beat hand-optimized
  machine code (`Tools/Perf/AsmParity`, `Jet/ceiling <= 1.00` in cycles and
  bytes) while delivering the same safety/diagnostic features. Reported now;
  gating opens when the owner enables it.
- **Cold compile speed:** Target is 30M LOC/s from cold. Measure live from cold;
  compile caches are off by default.
- **General root fixes only:** Fix losses in the general compiler/runtime/Core
  path and prove the gain on a second, different program. Never add
  benchmark-specific special cases, average away losses, drop peers, or trade
  correctness, safety, diagnostics, or I9 parity for a score.
- **Perf-motivated syntax:** Any surface added for performance requires a paired
  two-program cell proving it strictly beats the plain spelling before
  ratification.

## Git, Tower, and resource limits

- **Owner gates vs. autonomous work:** Put choices only the owner can make (new
  syntax, public API, commands, dependencies, invariant/I9 exceptions, scope or
  epoch moves, visual/product direction) on a Tower ballot with alternatives,
  trade-offs, and edge cases. Decisions live only on ballots—never in card notes
  or plans. Within approved scope, make implementation choices and keep moving;
  do not stall work to ask the owner implementation questions.
- **Tower hygiene and honest closure:** Use `node Tools/Tower/tower.mjs` (never
  hand-edit `Tools/Tower/.tower/`; only the owner starts `tower serve`). Workers
  never write to Tower, close cards, or keep a competing task list. Attach
  regressions and follow-ups as criteria on the owning card rather than creating
  new cards. A card closes only when its work is truly done; closed cards are
  never reopened. Meet a criterion only when evidence proves its full text
  (every boundary, tier, type, file, and sweep scope it names)—never on narrower
  evidence, a "close enough" substitute, or a known owed row. Never narrow,
  substitute, defer, or reword part of a criterion yourself; put any change on
  an owner ballot before marking it.
- **Branches, commit hygiene, and git safety:** Only three branches exist:
  `master` (live board + latest proven compiler), `dev` (active landing branch),
  and `rust` (frozen read-only reference). Never create extra branches, stashes,
  or ad-hoc refs (`refs/keep`, `refs/archive`, etc.). Never run broad or
  destructive git commands (`git add -A`, `git commit -a`, `git stash`,
  `git restore .`, `git reset --hard`) or overwrite another task's paths.
  Concurrent writers need disjoint paths. Every commit message body states what
  changed, why, and its proof. Never commit temporary directories, object files,
  or build output (`LaneCheck` refuses them).
- **Worktree and subagent caps:**
  - At most **5 worktrees** exist at any time (master checkout and `dev`
    worktree included; git hooks hard-cap at 10). Each worktree has one live
    owner, holds no commits of its own, and is removed once its work lands.
  - At most **3 subagents** run at a time (reviewers and helpers included).
    Workers do not spawn workers.
- **Machine safety:** Keep build output and scratch outside the repo using
  `Tools/Agent/roots.env`. Run at most one heavy job (compiler build, full
  suite, timed benchmark, release admit) on the machine at a time. Never let the
  scratch disk exceed 75% full; delete scratch once its work is landed on `dev`.

## Workflow, debugging, and verification

- **Collect-all, then fix in batches:** Prefer `--keep-going` / collect-all
  sweeps over serial one-error-at-a-time rebuilds. Before landing a stricter
  sema rule, guard, or gate, sweep every Jet source in the repo and fix all hits
  in the same batch.
- **Verification scoped to the claim:** Run commands with `jet` (`JET` =
  admitted native compiler). Use `jet build --profile dev Compiler` for fast dev
  rebuilds. Choose the proof that actually establishes the criterion: a code
  review for structural/cutover claims (copies deleted, one source, rule matches
  approved text); a targeted test/case for behavior; per-tier runs for tier
  claims ("every tier" includes web and comptime where applicable, or states
  explicitly why not); and a full suite for cross-cutting changes (sema rules,
  codegen, runtime, Core-wide cutovers, ledger removals). Record exact command,
  compiler identity, and result. A proof that silently skips on a missing tool
  or file proves nothing on that host.
- **Full suite cadence:** Land batches after a build and targeted proofs, but
  run the full suite (collect-all, compared with the last run) at least every
  few batches, and cleanly before any `a0.0.N` release. A card with a
  cross-cutting change closes only after a full suite that includes it. Local
  gates: `Tools/Agent/LaneCheck.jet` and `Tools/Agent/VerifyFull.jet`.
- **Honest evidence and known-failing ledger:** Evidence states what was proven,
  nothing more: compute metrics as defined (a sweep where every hit was correct
  code is 100% false positives); state exact counts, never "38+"; cite only
  tests that exercise the claim; and separate delivered criteria from criteria
  moved to another card in closing logs. Removing a `Tests/known-failing.tsv`
  row requires a real passing run on the batch compiler. Changing an expected
  output to match current behavior requires a stated reason checked against the
  spec or another primary source. Rows marked "owner question" are resolved only
  by the owner.
- **Dogfood Jet tools:** Debug Jet programs (including the compiler) with
  `jet debug` and profile with `jet perf`. Use external tools (`gdb`, `lldb`,
  `perf`, `strace`, `valgrind`) only as benchmark baselines or for a specific
  missing capability—and record the gap on the dogfooding card first.
- **Design review before build (binding cutover):** Solve the full mechanism
  with one source of truth and a complete cutover list, not a patch for one call
  site. Every design card (not only those claiming superiority) lands its review
  in `Docs/Audits/` in a commit before the build commits. The review names the
  strongest existing solutions with sources, attacks every claim, ends with a
  verdict, and turns every gap into a criterion on the card or a ballot. Its
  cutover list (copies to delete, registry text to reword, snapshots, sweeps) is
  binding—the card cannot close while any item remains.

## References

Load only what the task needs:

| When the task involves | Read |
|---|---|
| Multi-agent orchestration, workers, worktrees, batch landing, closure | `.agents/skills/orchestration/SKILL.md` |
| Vision and design priorities | `Docs/Spec/philosophy.md` |
| Language / compiler semantics & decision rationale | Code + tests, `Docs/Spec/spec.md`, `Docs/Spec/syntax-decisions.md` |
| Diagnostics and UI snapshots | `Compiler/Prelude/Diagnostics.jet`, `Docs/Spec/diagnostics.md` |
| Debugging or profiling Jet code | `.agents/skills/diagnosing-bugs/SKILL.md` |
| Examples and golden outputs | `Docs/Spec/Contributing/examples.md`, `Examples/README.md` |
| Tower board operations and triage | `Tools/Tower/skills/tower/SKILL.md`, `Docs/Spec/Contributing/issue-tracker.md`, `Docs/Spec/Contributing/triage-labels.md` |
| Domain vocabulary, security, engineering traps | `Docs/Spec/vocabulary.md`, `Docs/Spec/Contributing/domain.md`, `Docs/Spec/Contributing/security-closure.md`, `Docs/Spec/Contributing/agent-engineering.md` |
| Skill routing (audits, research, cleanup) | `.agents/skills/JetSkillsRouter.md` |
| Completion claims and milestone closeout | `.agents/skills/verify/SKILL.md` |
