# Owner Guidance

**Status:** Owner-maintained agent policy. Read before using a skill or dispatching an agent. Agents must not edit this file. Owner edits it through Tower's **Guidance** tab.

This is the only owner-edited source for shared agent conduct and model routing. It is not a spec, planner, phase model, or work ledger. Provenance and pre-compression wording live in `docs/agents/owner-guidance-evidence.md`; do not load that archive during normal work.

## Authority

Apply current owner instruction, ratified Tower decisions and acceptance terms, relevant domain specs, `AGENTS.md` invariants I1–I9 and owner gates, this guide, then task-specific mechanics and skills. Newer higher authority wins; never average conflicts.

Design belongs in specs and ballots. Work state belongs in Tower. Project state and technical traps belong in `docs/agents/agent-memory.md`. Dispatch mechanics belong in `docs/agents/orchestration.md`. Shared conduct and model choices belong only here.

## Operating contract

1. **Measure results, not activity.** A card is closed only when a fresh Tower query shows `done`, every observable criterion has concrete evidence from the integrated tree, and no known blocker contradicts it. Launches, code-complete claims, merges, green checks, and running proof are not closure.
2. **Do not stop with work in flight.** Harvest, integrate, prove, close, repair, and refill while work remains. Pause only a slice blocked on owner choice or external dependency; continue independent work.
3. **Keep owner choices explicit.** Questions are not approval. Ballot new syntax, public APIs, commands, dependencies, invariant exceptions, scope moves, and other owner gates. Show same-program alternatives with exact syntax, behavior, trade-offs, edge cases, and expert control.
4. **Plan before dispatch.** Each card and brief states actual and expected behavior, exact writable paths, last integrated state, dependencies, non-goals, invariants, full cutover, observable criteria, proof command, and compact return shape. Use a short goal for a capable planner; give bounded implementers complete context.
5. **Fix mechanisms, not symptoms.** Census before streaming discovery. Deduplicate by root cause, retarget surviving cards when cause changes, record absorbed work, and remove empty status-quo cards. Never suppress, special-case, weaken tests, or hide failure behind fallback or panic.
6. **Keep roles clear.** Orchestrator owns judgment, planning, briefs, dispatch, integration, proof, evidence, Tower, and closeout. Worker owns one coherent source slice and returns patch or commit, source evidence, lane-check receipt, and blockers. Workers do not write Tower, claim unrun proof, run broad tests, or spawn workers.
7. **Use adaptive concurrency.** Start with one delivery stream. Expand only when work has disjoint paths and tests, a clean integration target, enough machine and model capacity, and one close owner. Contract around shared seams, build contention, dirty ownership, memory pressure, or reintegration cost. Historical lane counts are not policy.
8. **Protect ownership.** Inspect current ownership and diff before writing. Never overwrite another task's paths; never use `git add -A`, broad `git commit -a`, `git restore .`, or hand-edit `plugins/tower/.tower/`. Use only in-repo worktrees under `.claude/worktrees/` or `.agent-worktrees/`. Integrate useful work promptly, salvage coherent work from dead workers, then remove finished worktrees and branches.
9. **Prove observable behavior.** Source inspection cannot prove runtime, tier, snapshot, golden, generated artifact, or data correctness. Run the smallest proof that can reject the integrated patch. Cover real boundaries, failure behavior, and silent-data risks. Apply I4 diagnostics, I5 executable examples, and I9 parity across every applicable tier; never park parity in `jit_gaps`.
10. **Separate card and milestone gates.** Close a card immediately when integrated criteria are met. Do not require a per-card reviewer or duplicate reassurance proof. After all milestone cards close, run one composed targeted sweep and one fresh-context integrated-diff review. Reopen owning cards for material findings. Run the full suite once at epoch end unless a known interaction requires earlier proof.
11. **Keep Tower honest.** Query before quoting counts. Every card has one home, implementable plan, observable criteria, dependencies, and current phase. Use non-serve CLI against main checkout. Only owner starts Tower server; report stale state instead of starting another server.
12. **Communicate measured state.** Work autonomously when unblocked. Group necessary questions. Report exercised behavior, newly closed card IDs, blockers, owner gates, regressions, resource faults, and next action. Never claim readiness from internal implementation alone. Keep private chat content private; use plain professional prose for durable artifacts.
13. **Protect machine.** `/tmp` is RAM-backed. Keep scratch in `~/.cache/jet-test-scratch`, briefs and logs in configured disk cache, one shared bounded target, and `CARGO_INCREMENTAL=0`. Monitor RAM, swap, disk, target size, and process liveness. Use `scripts/agent/disk-report.sh` and `scripts/agent/proof-parallel.sh`; default `JET_TARGET_CAP_GB` is 120. Fix root resource defects rather than raising guards.
14. **Improve whole product.** Prefer one systemic mechanism over benchmark, example, or language-specific patches. Preserve safety, determinism, tier parity, performance, beginner defaults, and explicit expert control. Optimize first for reasoning, then reading, then writing. Use structural prevention—diagnostics, lints, safe fixes, formatter or language-server actions, and executable examples—over recurring prose warnings.
15. **Keep Jet human-first.** Public work targets people and broad workloads. Agent usability is a consequence, not product positioning. Clear defaults, commonality-weighted friction, realistic examples, and visible terminal or UI feedback are product quality.

## Shared routing and skill consolidation

Tower decision `D-AGENT-SKILL-CONSOLIDATION1` ratified option A on card `#2401`:

- This file owns shared conduct, model adapters, routing, and retirement state.
- `docs/agents/orchestration.md` keeps mechanics only and names capabilities, not models.
- Repo task skills keep distinct domain steps.
- `.claude/skills/burndown/SKILL.md` retires.
- `tower-burndown` remains a thin Tower adapter.
- `jet-fast-burndown` and `milestone-burndown` are disabled inside Jet, not deleted globally.
- Dispatch must fail closed when an adapter is missing, stale, conflicting, or unknown.
- Generated drift lock covers active repo, managed, plugin, vendor, and cache inputs; vendor files remain read-only.

Migration is in progress. Until each route below is resolved, latest explicit owner instruction wins. Without one, do not invent a model choice or silently select a conflicting workflow.

| Concern | Canonical route | Status |
|---|---|---|
| Burndown roles and closure | `tower-burndown` owns Tower scope and board operations. `orchestration.md` owns dispatch, concurrency, worktrees, integration, proof cadence, and closure law. Plain burndown requests authorize workers. Project `burndown` is retired; managed duplicates are disabled inside Jet. | Resolved 2026-08-31 |
| Review routing | User-requested branch, PR, or fixed-point review uses `code-review`. Internal terse bug scans use `cavecrew-reviewer`. Tower milestone closeout uses a dedicated fresh-context milestone review under `verify`. `codex-result-handling` transports Codex findings only and never defines review scope or blocks milestone repair. | Resolved 2026-08-31 |
| Research and reports | No consolidation. Keep all audit, research, and report skills unchanged with their current triggers and methods. `show-html` runs only when owner explicitly invokes or requests an HTML version; never auto-convert another skill's output. | Resolved 2026-08-31 |
| Model routing | Code-writing workers use GPT-5.6 Luna with max reasoning. Full `code-review` axes and Tower milestone reviews use GPT-5.6 Sol with high reasoning. All cavecrew roles use Sonnet, not Haiku. Audit, research, report, `show-html`, and gauntlet routes remain unchanged. Unknown, unavailable, or conflicting profiles fail closed. | Resolved 2026-08-31 |
| Codex invocation | OMP `task` and `hub` are primary for every dispatch. Select the most specific OMP agent; `~/.omp/agent/config.yml` role aliases choose model and reasoning. Direct Codex CLI and `codex-rescue` are fallbacks only after OMP cannot run the required agent, except unchanged specialized skills that explicitly own direct invocation. `codex-cli-runtime`, `gpt-5-4-prompting`, and `codex-result-handling` remain internal to rescue. | Resolved 2026-08-31 |

### Active adapter profiles

| Capability | OMP role | Model | Reasoning | Agents |
|---|---|---|---|---|
| Implementation | `@implementation` | GPT-5.6 Luna | max | `task` and normal code-writing workers |
| Full review | `@full_review` | GPT-5.6 Sol | high | `reviewer`, `security-reviewer`, `code-review` axes, Tower milestone review |
| Cavecrew | `@cavecrew` | Sonnet | host profile | `cavecrew-investigator`, `cavecrew-builder`, `cavecrew-reviewer` |
| Specialized audit/research/report | Skill-defined | Skill-defined | Skill-defined | Existing audit, research, report, explicit `show-html`, and gauntlet workflows |

Main orchestrator keeps active session model. Full ballots still require a real rival model family for adversarial review. Dispatch stops when resolved model or reasoning level differs from this table. OMP agent harness is mandatory first path; any CLI fallback records exact harness failure and reason fallback can satisfy task.
