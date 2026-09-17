# Skill sources and dispositions

Consult this inventory for installation, discovery, and retirement work, not routine task routing. The operational index is [JetSkillsRouter.md](../JetSkillsRouter.md). This is not a second authority. `AGENTS.md`
controls shared conduct, model adapters, precedence, and retirement state.
Each row preserves the skill's trigger and unique rule. Shared conduct inside a
skill yields to the owner guide.
Source keys:

- `R` — `.agents/skills/<name>/SKILL.md`.
- `T` — `plugins/tower/skills/<name>/SKILL.md`.
- `M` — `/home/nate/.omp/agent/managed-skills/<name>/SKILL.md`.
- `H` — host plugin or cache copy under `$HOME/.claude` or `$HOME/.codex`.
- `G` — `/home/nate/.agents/skills/<name>/SKILL.md`.
- `A` — advertised in the audited runtime catalog, not proof of current invocation behavior; `F` — filesystem-backed entry.

### Entry map

| Reach | Source | Skill | Status | Trigger | Unique rule preserved or Jet boundary |
| --- | --- | --- | --- | --- | --- |
| A | — | `burndown` | retire | “burn down”, “close cards”, epoch, or multi-card wave | Stale generic route. Route those requests to `tower-burndown`. No current file. |
| F | R | `ask-matt` | retire | Ask which skill or flow fits | Old skill router. `jet-router` is the one Jet router. |
| F | R | `batch-grill-me` | keep | Batch interview or frontier questions | Ask every settled question in one round, then wait for answers. |
| A+F | H | `cavecrew` | keep | Delegate, use Cavecrew, save context, or request investigator, builder, or reviewer | Select the narrow Cavecrew preset and return compressed output. |
| A+F | H | `caveman` | keep | `/caveman`, caveman mode, or owner-wide compressed chatter | Compress communication while preserving facts; mode persists until explicit stop. |
| A+F | H | `caveman-commit` | keep | Write or generate a commit message, or staging auto-trigger | Conventional Commit subject at most 50 characters; add a body only when needed. |
| A+F | H | `caveman-compress` | keep | `/caveman-compress FILEPATH` or compress a memory file | Preserve code, URLs, and technical meaning; keep a human-readable backup. |
| A+F | H | `caveman-help` | keep | `/caveman-help`, “caveman help”, or command help | One-shot reference. Do not change mode or write state. |
| A+F | H | `caveman-review` | keep | Review a diff in compressed form | One line per finding: location, problem, fix. |
| A+F | H | `caveman-stats` | keep | `/caveman-stats` | Read exact session-log usage through the hook; do not estimate. |
| A+F | R | `code-review` | narrow | Review a branch, PR, WIP, or changes since a fixed point | Keep parallel Standards and Spec axes; owner guide chooses review adapter and model. |
| A+F | R | `codebase-design` | keep | Design a deep module, interface, seam, or testable boundary | Use the module, interface, seam, leverage, and deepening vocabulary. |
| A+F | H | `codex-cli-runtime` | narrow | Only inside `codex-rescue` | One companion task, stdout unchanged, no follow-up or project policy. |
| A+F | H | `codex-result-handling` | keep | After Codex helper output | Preserve verdict, evidence, and finding boundaries; do not imply a fix. |
| A+F | R | `diagnosing-bugs` | keep | “diagnose”, “debug”, broken, throwing, failing, or slow | Reproduce, isolate the root cause, and keep a feedback loop. |
| A+F | R | `domain-modeling` | keep | Domain terms, ubiquitous language, glossary, or ADR work | Challenge terms and record the model when it crystallizes. |
| A+F | R + M | `eli5` | keep | ELI5, “explain simply”, plain English, or beginner explanation | Build an accurate beginner mental model. Select the canonical `R` source, not the managed duplicate. |
| A+F | R + M | `eli5-caveman` | keep | ELI5 plus brief or caveman compression | Explain accurately, then compress without dropping mechanisms or material caveats. Select `R`, not the managed duplicate. |
| A+F | G | `find-skills` | narrow | Find a skill, ask how to do something, or discover capability | Discover host capabilities only; `jet-router` still routes Jet work. |
| A+F | R | `first-principles-audit` | keep | “first principles audit”, “rethink from first principles”, or “find the unifying idea” | Sweep the corpus and silhouette, then produce a unifying proposal and ballot slate. |
| A+F | H | `frontend-design` | keep | New UI, frontend, visual design, or UI reshaping | Make deliberate visual, typographic, responsive, and accessible design choices. |
| A+F | R | `garbage-collection` | keep | Dead code, stale docs, plans, or agent outputs | Prefer deletion of dead machinery and stale artifacts. |
| A+F | R | `gauntlet` | narrow | Gauntlet run, build, update, competitive corpus, or scoreboard | Keep paired real programs, matrix, harness, and win/parity/loss report; owner guide controls agents and models. |
| A+F | H | `gpt-5-4-prompting` | narrow | Compose a Codex prompt for coding, review, diagnosis, or research | Keep task, output, verification, grounding, and safety blocks; rescue-only. |
| F | R | `grill-me` | retire | `/grill-me` or sharpen a plan or design | Alias for `grilling`; use the canonical interview skill. |
| F | R | `grill-with-docs` | narrow | Interview plus ADR or glossary capture | Keep the combined interview/document result; use bounded `/grilling` or requested `/batch-grill-me` plus `/domain-modeling` handoffs that return to this owner, with no cycles or follow-on agenda. |
| A+F | R | `grilling` | keep | Grill, stress-test, or challenge a plan, decision, or idea | Ask one decision question at a time and wait before acting. |
| F | R | `handoff` | keep | Handoff to another session or agent | Write a compact temporary handoff and point to durable artifacts instead of copying them. |
| A+F | M | `implementation-before-validation` | narrow | Explicit owner instruction to implement before validation | Apply only to the approved phase ordering. Preserve honest unrun-proof status; use the current authority when ordering conflicts. |
| F | R | `implement` | narrow | Implement work from a spec or ticket | Keep implementation intake; owner guide controls worker scope, checks, proof, and closure. |
| F | R | `improve-codebase-architecture` | narrow | Improve architecture or find deepening opportunities | Keep the scan and chosen deepening path; bounded grilling/design handoffs return to this owner and never jump to implementation. |
| A+F | R | `isomorphic-ontology-audit` | keep | Ontology, isomorphism, concept unity, or false-rhyme review | Map each surface form to its foundational concept; use probe and honesty sections only. |
| A+F | M | `jet-fast-burndown` | disable | Managed high-throughput burndown workflow | Preserve the global installation; Jet must not load its conflicting lane and closure rules. |
| A+F | R | `jet-router` | keep | Unclear skill, pulse, or health check | Select one primary Jet outcome; permit only declared bounded handoffs that return to it, with no cycles or silent scope expansion. |
| A+F | R | `lessons-learned` | keep | Peer-language lineage, regrets, or do-not-repeat research | Convert peer failure to Jet risk and a guard such as an invariant, card, or ballot. |
| A+F | M | `milestone-burndown` | disable | Card-first closure, milestone sweep, or milestone verification | Preserve the global installation; Jet uses owner-guide closure law through `tower-burndown`. |
| A+F | R | `mine-for-jet` | keep | Mine a repository, video, article, paper, docs site, or discussion | Capture claims and evidence, cross-check Jet state, and preserve syntax, API, UX, and tooling details. |
| A+F | R | `mission-audit` | keep | Mission, philosophy, or language-experience alignment | Score beginner defaults, expert control, safety, diagnostics, batteries, and tooling. |
| F | R | `orchestration` | keep | Dispatch, waves, worktrees, receipts, integration, recovery, or card closure | Apply the shared contract through bounded briefs, disjoint ownership, focused proof, and immediate Tower closure. |
| A+F | R | `persona-audit` | keep | Persona status, practical use, push, pull, or development feel | Use fresh personas, representative runs, and concrete push/pull evidence. |
| A+F | H | `ponytail` | keep | Coding, refactoring, fixing, reviewing, or minimal design | Apply YAGNI, existing mechanisms, stdlib, and native features before adding code. |
| A+F | H | `ponytail-audit` | keep | Whole-repo over-engineering audit, bloat, or deletion review | Return ranked cuts; do not apply fixes. |
| A+F | H | `ponytail-debt` | keep | “ponytail debt”, shortcut ledger, or deferred work | Harvest every `ponytail:` comment with its ceiling and upgrade path. |
| A+F | H | `ponytail-gain` | keep | Ponytail impact, savings, or scoreboard | Show the one-shot measured scoreboard without changing state. |
| A+F | H | `ponytail-help` | keep | `/ponytail-help` or Ponytail command help | One-shot reference; do not change mode or persist state. |
| A+F | H | `ponytail-review` | keep | Review for over-engineering, needless deps, or what to delete | Findings only: location, cut, replacement. |
| A+F | R | `pragmatism-audit` | keep | Real-work friction, missing defaults, or “does this help me finish?” | Test beginner magic and explicit expert reject or override across workloads. |
| A+F | R | `prototype` | keep | Sanity-check a state model, logic path, or UI | Build throwaway code to answer one question, then capture the answer and discard the shell. |
| A+F | R | `research` | keep | Research a topic, docs, API fact, or source-backed question | Use high-trust primary sources and one cited Markdown result. |
| A+F | R | `resolving-merge-conflicts` | keep | In-progress merge or rebase conflict | Trace both intents, resolve every hunk, and never abort the merge. |
| A+F | R | `rli5` | keep | `rli5`, “read like I’m five”, beginner lens, or learnability review | Read as a genuine newcomer and report where the artifact fails to teach itself. |
| A+F | R | `html` | narrow | Explicit `/html`, “show as HTML”, or an explicitly requested visual HTML artifact | Keep self-contained zero-dependency HTML in the skill's jet-black/red identity; do not auto-convert another skill's output. |
| A+F | R | `simple` | keep | Docs, specs, ballots, reports, commits, or `simple`/`STE` request | Use clear controlled prose while preserving technical tokens, diagnostics, quotes, and ratified wording. |
| F | R | `setup-matt-pocock-skills` | retire | Set up the generic engineering-skill tracker or docs layout | Jet already has Tower and its own domain layout; keep this one-time installer out of the Jet route. |
| A+F | R | `spec-compliance-audit` | keep | Ratified syntax/spec versus parser, sema, tests, or examples | Measure shipped, partial, gap, gated, declined, and stale-doc states; do not reopen syntax. |
| F | R | `pulse` | keep | Status, how close to done, or mid-run progress check | Short ELI5 snapshot; estimated task and work-weighted overall completion; proof before 100%; no unsolicited reports or work interruption. |
| A+F | R | `structure-cleanup` | keep | Restructure files or remove indirection without behavior change | Change navigation and cohesion only; stop when semantics, APIs, diagnostics, or goldens would change. |
| A+F | R | `surface-audit` | keep | Language shape, uniformity, consistency, syntax, or structure outliers | Find outliers and gaps, then state concrete next actions. |
| A+F | R | `surface-frequency-audit` | keep | Frequency of language features, APIs, idioms, tooling, or public code surfaces | Measure real code, preserve beginner and expert paths, resume safely, and report without Tower writes. |
| A+F | R | `surface-research` | keep | Research peer languages for Jet surface ideas | Record useful ideas, Jet use, and the failure mode to avoid. |
| A+F | R | `tdd` | keep | Test-first, red-green-refactor, or requested integration tests | Keep the behavioral test loop and anti-pattern guidance. |
| F | R | `teach` | keep | Teach a skill or concept over multiple sessions | Use workspace learning records and continue from the learner's state. |
| F | R | `to-spec` | narrow | Turn the current conversation into a spec | Keep synthesis without an interview; publish only through the configured Tower route and stop without an automatic implementation agenda. |
| F | R | `to-tickets` | narrow | Break settled implementation work into tracer tickets | Keep blocking edges and slicing; reject unresolved design questions and keep Tower as work-state authority. |
| F | T + H | `tower` | keep | Read, claim, update, or otherwise work the Tower board | Keep CLI-backed board mechanics; never hand-edit board data or start a second server. |
| F | T + H | `tower-ballot` | keep | Queue a decision or make a choice ballot-ready | Keep gist, story, in-the-wild example, worked options, recommendation, and loss reasons. |
| F | T | `tower-burndown` | narrow | Burn down, close out, work the backlog, epoch, or sidequest | Keep Tower scope and board operations; owner guide supplies dispatch, integration, proof, and closure mechanics. |
| F | T | `tower-prep` | keep | Prepare cards, plans, ballots, or a burndown queue | Keep planning and decision exposure; do not implement or close cards. |
| F | T | `tower-rank` | keep | Rank, reorder, triage, thin, or choose next Tower work | Keep dependency-safe `workOrder`; do not plan, implement, review, verify, or ballot. |
| F | T + H | `tower-setup` | narrow | Set up Tower, configure it, or fix first-run state | Keep setup and config inspection; only the owner may start the board server. |
| F | R | `triage` | narrow | Triage issues or external PRs and write briefs | Keep the triage state machine; use Tower for all Jet issue state and owner decisions. |
| A+F | R | `type-unification-audit` | keep | “Should X be a type?”, marker/type disjointness, or meta-type review | Find phantom types and fragmented facts; do not reopen syntax while measuring the type model. |
| A+F | M | `unslop` | disable | Human-sounding prose cleanup or AI-writing-tell removal | Preserve the global installation; Jet's canonical prose route is `simple`. |
| A+F | M | `unslop-caveman` | disable | Unslop plus compressed or caveman prose | Preserve the global installation; Jet uses `simple` plus active `caveman` policy. |
| A+F | R | `verify` | keep | Code done, snapshot/golden closeout, or false-green check | Own criteria evidence, fresh-binary proof, snapshots, goldens, and resource traps. |
| F | R | `wayfinder` | narrow | Plan work larger than one agent session or map a destination | Keep decision-map reasoning; bounded supporting results return to wayfinder, with no implementation jump or competing planner. |
| F | R | `writing-great-skills` | keep | Write, edit, or evaluate a skill | Keep predictability, invocation metadata, completion criteria, progressive disclosure, and host-support caveats. |

## Project discovery

`.omp/config.yml` selects `.agents/skills` and `plugins/tower/skills` as canonical custom directories and excludes the four disabled logical names. The canonical ELI5 files override same-named managed copies; do not name-ignore ELI5 itself. Leave global settings and managed, installed-plugin, vendor, and cache sources untouched. Canonical in-repository Tower sources are editable when the owner requests their maintenance.

Check the effective name-to-file mapping with the installed host's real loader after discovery changes. The source checker only validates the inventory and source constraints; it does not prove routing behavior or enforce write permissions. Unmatched host catalogs have no Jet-specific route.
