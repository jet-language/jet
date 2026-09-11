# AGENTS.md — Jet shared agent contract

This is Jet's sole cross-tool policy. `CLAUDE.md` is its symlink. Strategic guidance lives in `docs/spec/philosophy.md`; plans and work state live only in Tower. Procedures belong in skills. Code, executable registries, and exercised tests/examples establish current behavior.

## Authority and mission

Read `docs/spec/philosophy.md` for purpose and ranked design priorities. Do not copy that guidance into other documents.

Code is the source of truth for what Jet does. Specs explain durable contracts and reasons; they do not prove that a feature works, is absent, or is complete. A conflict with an approved requirement is a defect to investigate and home in Tower, not permission to bless the implementation or repeat stale prose.

For decisions about intended behavior and agent conduct, resolve conflicts in this order:

1. The owner's latest explicit instruction.
2. A ratified Tower decision and its acceptance terms.
3. The relevant domain specification or ADR.
4. This contract.
5. The task-specific skill.

A newer higher-authority ruling wins. Do not average conflicting rules. Stop only the affected slice when a conflict or owner gate blocks it; continue independent work.

## Decision Level Definitions

### Strategic

Defines purpose and direction: what we want to achieve, why it matters, and what
principles and trade-offs should guide us. Strategic discussion establishes
desired outcomes, priorities, boundaries, and what success means. It can question
whether an undertaking is worth pursuing at all.

Central question: What are we trying to accomplish, and why?

### Operational

Translates strategic direction into a coherent approach and organized work.
Operational discussion determines what needs to exist or change, how the major
pieces fit together, and how to sequence and coordinate them. It covers
substantial design choices, scope, dependencies, milestones, and acceptance
criteria without resolving every implementation detail.

Central question: What approach and body of work will achieve the intended
outcome?

### Tactical

Concerns the concrete actions and decisions that carry out the operational
approach. Tactical discussion resolves implementation details, performs specific
changes, handles immediate problems, and verifies results. Findings at this level
can reveal that an operational approach or strategic assumption needs
reconsideration.

Central question: What specifically do we do, and how do we establish that it
worked?

## Documentation boundaries

- **One strategic document:** `docs/spec/philosophy.md` holds durable vision and design priorities. No schedules, feature inventories, progress notes, or release-by-release scope. Do not create a separate roadmap without owner direction.
- **One planning system:** Tower holds specific goals, plans, milestones, dependencies, decisions, acceptance criteria, status, blockers, and handoffs. Move still-live work there before removing its document copy; discard superseded plans rather than preserving a second queue.
- **Executable truth:** Keep spellings, APIs, diagnostics, defaults, and constraints in their code or registry home. Use tests, snapshots, and executable examples to prove behavior. Generate reference output on demand; do not check generated status or duplicate catalogs into docs.
- **Small explanations:** Specs and guides may explain durable contracts, non-obvious reasons, and how to use or change the system. Link to executable sources instead of maintaining feature lists, limitations, coverage tables, or implementation maps. Fix stale prose by cutting it or correcting the explanation, never by redefining reality.
- **Evidence, not work state:** Audits and research retain dated findings; proposals retain alternatives and reasons linked to a Tower decision. They cannot own plans, goals, or a live status record. Historical evidence does not establish current behavior.

Docs have four categories: `spec`, `audits`, `research`, and `proposals`; `docs/README.md` is navigation only. Only the owner may delete audits or final reports; agents may move them intact. Do not create cleanup reports, preservation maps, archive buckets, or recurring documentation janitors. Preserve unique decision rationale in its existing home; use Tower and source history for work and recovery.

## Greenfield cutover

Jet has no compatibility obligation to repository history until the owner declares one. Ship one canonical current form. When syntax, semantics, APIs, ABIs, formats, names, or commands change, migrate every in-repository caller, example, test, generated artifact, schema, and affected explanation in one cutover. Remove old spellings, aliases, shims, fallback parsers, legacy readers, version branches, and parallel implementations. Keep ratification history in Tower and only necessary design rationale in specs, not retired behavior in compiler or tools. A compatibility exception needs an owner-ratified decision with exact scope and removal condition.

## Invariants

A violation stops the affected work and requires a root fix.

- **I1 — Safety.** Jet is memory-safe and type-safe by default. Expert escape uses user-written audited `#Unsafe("reason") { … }` or `#Unsafe("reason") fn` regions. Generated Rust `unsafe` is allowed only there or in vetted standard-library and memory internals.
- **I2 — Hidden backend.** A rustc rejection of generated code is an internal compiler error with exit 101, never a user diagnostic.
- **I3 — Sema owns checks.** All language checking lives in sema. Codegen lowers known facts; it never probes rustc to discover user errors.
- **I4 — Diagnostic product.** Every diagnostic has a registered code, what/why/fix text, and a UI snapshot. Without the snapshot, the diagnostic is incomplete.
- **I5 — Executable example.** Every feature has an example and golden-tested output. The example proves the same meaning on every applicable execution tier.
- **I6 — Dependency seams.** The compiler and compiler seam crates use path dependencies only. Existing ratified stdlib bootstrap dependencies are temporary. A new stdlib external dependency requires an owner decision and an approved bridge pattern.
- **I7 — Ratified syntax.** Every user-typeable keyword or sigil is registered in `crates/jet-foundation/src/Syntax.rs` and tied to a decision ID.
- **I8 — One mechanism.** Keep one canonical semantic mechanism, with flexible spelling or organization only where it improves use. Keep the beginner surface small and safe. Expose expert control by explicit opt-in. A new mechanism needs approved Tower scope or owner approval.
- **I9 — One meaning across tiers.** AOT, Cranelift JIT (`jet run` and `jet dev`), interpreter/deopt, and web when applicable preserve one executable meaning for every language feature and Core API. Semantics live in the embedded Prelude and ratified CoreLib. AOT emit, JIT hosts, and interpreter ambient marshal values and call the same Prelude symbols; they must not re-encode validation, defaults, policy, or error meaning. Do not mark a feature AOT-only, add a new `tests/jit_gaps.txt` parking entry, or say “JIT later.” Prove AOT and default `jet run`; prove interpreter and web when the surface reaches them. An owner-ratified exception must name the inapplicable tier.

## Strict performance gate

Jet targets a strict win for every matched peer on every required cell and metric. Rust permits same-run parity only at a Jet/Rust ratio of `1.05` or lower; that band is measurement noise, not a target or a win. Every non-Rust peer requires Jet/peer below `1.00`.

Required cells cover foundations (numerics, text, files, concurrency, networking, build time, and run time) plus one real workload in each critical area: web, games, CLI and scripts, data analysis, backend services, AI/ML applications, GUI applications, and embedded. A niche becomes required only when Jet ships a first-party battery for it. Do not claim a niche win without that battery.

Apply the comparator per cell and metric. Never average away a loss, substitute an easier workload or tier, omit a peer, or pass wrong, unavailable, uncovered, mismatched, or inconclusive evidence. A performance card, milestone, dashboard, or release gate stays open while any required cell fails. Never trade semantics, diagnostics, determinism, safety, or I9 parity for a score. Manifests and gates must encode this policy; prose alone is not evidence. Historical receipts remain immutable evidence under their recorded policy and never weaken the current gate.
- A performance-motivated surface must arrive with a paired two-program cell against the plain spelling it replaces; ratification waits for a strict surface/plain win. Record the pair in the canonical manifest and keep any loss carded; this references the comparator above.

## Owner gates and work state

Before coding, identify genuine owner-only choices: new syntax, public API, command, dependency, invariant or I9 exception, epoch or scope move, product behavior, or visual direction. Make each a Tower ballot with same-program alternatives, exact syntax and behavior, trade-offs, edge cases, and beginner and expert paths. Do not ballot implementation choices already covered by an approved contract. A question is not approval. A ratified outcome remains law until the owner changes it.

Tower is the only work ledger. Every incomplete stream has one homed card with current phase, plan, dependencies, criteria, and handoff state. Workers never write Tower, close cards, or maintain a competing task ledger. The orchestrator integrates a valid result, runs the exact focused proof named by the criteria, records evidence, closes the card, and confirms `done` before claiming or briefing more work. A milestone then gets one commit-bound composed sweep and one fresh-context review; a card does not wait for that later gate.

One implementer owns each coherent patch. Concurrent writers use disjoint paths and one named close owner. Default to one delivery stream; add streams only when paths, integration, tests, and resources are clean. Use only in-repository worktrees under `.claude/worktrees/<name>` or `.agent-worktrees/<name>`, share the bounded main `target/`, integrate promptly, and remove finished worktrees and temporary branches. Never overwrite another task's paths. Never use `git add -A`, broad `git commit -a`, `git restore .`, or an equivalent broad operation. Never hand-edit `plugins/tower/.tower/`.

The owner alone starts `tower serve`. Agents use non-serve Tower CLI commands against the main board and report a stale or duplicate server. Never touch owner personal notes or scratch files.

## Routing and model contract

OMP `task` and `hub` are the first path for every dispatch. Use the most specific available agent. Main keeps its session model. Active profiles:

- `@implementation`: GPT-5.6 Luna, maximum reasoning, for normal code-writing workers.
- `@full_review`: GPT-5.6 Sol, high reasoning, for full review axes, security review, and milestone review.
- `@cavecrew`: Sonnet for investigator, builder, and reviewer roles.
- Existing audit, research, report, HTML, and gauntlet skills keep their own declared routes.

Managed duplicate skills are disabled inside Jet.

A missing, stale, unknown, or conflicting adapter fails closed. Direct Codex or rescue CLI is a fallback only after OMP cannot run the required role; record the exact harness failure in `JET_OMP_FALLBACK_REASON`. Workers do not spawn workers. Every code worker returns the exact `CHECK OK` receipt from `scripts/agent/lane-check.sh`; prose or static-data-only work returns `DOCS ONLY`. The orchestration skill defines briefs, liveness, integration, proof cadence, recovery, and closure mechanics.

## Environment and proof

Run repository commands through `scripts/agent/jet-env`. Keep scratch and logs on disk at `~/.cache/jet-test-scratch` and `~/.cache/jet-luna`, never in `/tmp`; `/tmp` is RAM-backed. Share one bounded Cargo target, set `CARGO_INCREMENTAL=0`, and respect the default `JET_TARGET_CAP_GB=120`. Rebuild before compiler smoke tests. Use the exact narrow criterion proof after integration. Broad suites, unfiltered censuses, and `verify-full.sh` are milestone or release operations only, with the required token. A worker type-check is not runtime, tier, golden, snapshot, or generated-artifact proof.

## Triggered references

Read this file first, then load only what the task needs:

| Trigger | Canonical reference |
|---|---|
| Dispatch, waves, worktrees, receipts, recovery, card closure | `.agents/skills/orchestration/SKILL.md` |
| Compiler or language semantics | Relevant code, executable examples and tests; `docs/spec/syntax-decisions.md` for decision rationale |
| Diagnostics or snapshots | `crates/jet-codegen/src/Prelude/Diagnostics.jet`, matching UI snapshots, and `docs/spec/diagnostics.md` |
| Examples or golden paths | `docs/spec/contributing/examples.md` and `examples/README.md` |
| Tower cards, questions, tags, or board operations | `docs/spec/contributing/issue-tracker.md`, `docs/spec/contributing/triage-labels.md`, `plugins/tower/skills/tower/SKILL.md` |
| Domain vocabulary or decisions | `docs/spec/vocabulary.md`, `docs/spec/syntax-decisions.md`, `docs/spec/contributing/domain.md` |
| Security closure | `docs/spec/contributing/security-closure.md` |
| Technical traps | `docs/spec/contributing/agent-engineering.md` |
| Audit, research, cleanup, or skill routing | `.agents/skills/JetSkillsRouter.md` |
| Code completion claims or milestone closeout | `.agents/skills/verify/SKILL.md` |

Use `ponytail` for coding and technical design: understand the whole path, then choose the smallest complete change. Use clear, plain prose for durable artifacts. Do not stop at a report when the task asks for implementation, and do not claim work that the board or an exercised command does not prove.
