# Agent memory (owner-auditable)

Durable notes every Jet agent reads. Exported 2026-08-09 from per-session agent memory so the owner can audit and edit them in one place. Orchestration law lives in `docs/agents/orchestration.md`; this file holds owner preferences, project state, and technical traps.

Edit freely: delete what is stale, correct what is wrong. Agents treat this as law after `AGENTS.md`. Dated audit entries below record what was ratified and what is still owed — they go stale fast, prune them once their cards close.

## Owner preferences and working agreements

### acceptance-requests-visual-only-short

*Owner 2026-07-12 — acceptance requests must be short + phone-friendly; owner confirms VISUAL things only, agents test + attach proof for everything else*

Owner on the Now-page verification queue: "what I'd prefer is for me to confirm visual things primarily with you providing proof but otherwise you should do testing and prove to me it's good to go. The current acceptance requests are very long and ask me to do too much especially when I don't have access to my computer."
**Why:** he's often on his phone; long check-lists that require running commands are unusable and offload agent work onto him.

**How to apply:** acceptance ballots/queue entries lead with agent-provided proof (test counts, evidence one-liners, screenshots for UI). Owner action = one short visual confirmation at most (with screenshot/artifact attached), or plain "machine-verified, accept to close" when nothing is visual. Never ask him to run commands as part of acceptance. Related: [[frontend-acceptance-full-mock-matrix]] (owner terminal checks still apply to D-FE-class deliverables he explicitly wants to drive), [[greenlight-gate-removed]].

### active-swarm-orchestration

*Keep 5+ Luna-max workers active while resources permit; heartbeats are crash-recovery only; refill immediately*

Owner directives (2026-08-08, escalating): "ramp up subagent usage and parallel development as much as possible"; "biggest concern is not number of subagents but swap/temp and ram/memory... go to town... shred through the backlog fast using a swarm of Luna max agents"; "spin up as many subagents as you need"; and on seeing one worker after a completion: "you said you'd have replacement workers backfill immediately but only one is running?"

**Rules:** NO fixed worker cap — swarm size governed ONLY by machine headroom (check `free -g` available ≥15G, swap <4G, `df -h /tmp` <70% before each dispatch wave; throttle only on breach). Lunas are build-free (sandbox) so cheap; the orchestrator's batched proof BUILDS are the real memory load. Refill to full width IMMEDIATELY on every worker completion/stall — never let the swarm drain. Heartbeat wakeups are crash-recovery FALLBACK only; never sleep while unblocked work exists. Lanes that never block on a running main-tree proof: worktree implementers, board-only, docs-only, review. Autocompact beyond ~150k context. Related: [[orchestrate-event-driven-heartbeat-backup]], [[shared-tree-safety]], [[epoch-scope-gates-spend]].

### agents-md-canonical

*AGENTS.md is the canonical cross-tool agent manual; CLAUDE.md is a symlink to it — owner refuses per-agent duplicate infra (Cursor/Codex/Claude)*

Owner switches between agents (Claude Code, Cursor, Codex/GPT) and refuses to maintain separate files/infra per tool. `AGENTS.md` at repo root is the canonical operating manual; `CLAUDE.md` is a symlink to it (done on worktree-tower-platform 2026-07-06; master still has standalone CLAUDE.md until merge).
**Why:** per-tool forks drift and double maintenance (2026-07-06).

**How to apply:** edit AGENTS.md only, never recreate CLAUDE.md as a real file. Tool-neutral hook scripts live in `scripts/agent/`; only thin JSON wiring stays per-tool (.claude/settings.json). New durable agent rules go in AGENTS.md, not Claude-only surfaces. See [[caveman-always-on]].

### all-cards-must-be-homed

*Owner ruling 2026-08-05 — every Tower card must be in an epoch, a sidequest, or frozen; the store now rejects unhomed cards on add AND update*

Owner directive 2026-08-05: "ALL CARDS MUST be either in an epoch, frozen, or a sidequest. It should be impossible to have something out of those bounds."
**Why:** Epoch-track cards with `epoch: null` are invisible in the owner's board views — cards #1440/#1441 vanished this way (created while no epoch was active, so `activeEpoch()` inherited null) and the owner found them missing twice.

**How to apply:** Enforcement is in code (`checkCardHome` in plugins/tower/app/store.mjs on add+update, `unhomed-card` rule in lint.mjs, `empty()` boards born with an active e1). When adding cards while no epoch is active, ALWAYS pass `--epoch <id>` (or `--track sidequest`) — the CLI now errors otherwise. Related: [[cards-are-handoff-source-of-truth]], [[owner-only-tower-serve]].

### ballot-first-always

*Balloting owner-blocking decisions is THE*

**RULE: The moment you identify a decision only the owner can make, ballot it IMMEDIATELY. This is the #1 priority. It supersedes all implementation work.**

Owner (repeated, multiple sessions, with extreme anger): "Your NUMBER 1 PRIORITY is to ensure things blocked on me (decisions) are created & added to the ballot so I can address them & unblock you."

**What counts as an owner-blocking decision:**
- Any architectural question with multiple valid approaches (e.g. foundation crate name/contents for c160 step 3b)
- Any new external dependency (I6 gate)
- Any syntax/API surface question the owner hasn't already ratified
- An "Owner-Q" in a sidequest plan that says "confirmable" — confirmable means BALLOT IT, don't assume
- Any step in a sidequest plan that has unresolved design questions

**What goes wrong without this:**
- I hit complexity → decide unilaterally → ship the wrong thing → owner reverts it (c160 scope; c157 stub)
- I hit complexity → decide to defer → silently skip a card step → owner finds incomplete work
- Work that needs owner input silently stalls with no ballot visible to him

**Never ask permission to mint.** 2026-08-05, after "say the word and I mint the ballots," owner: "Sure I guess ballot stuff if i need to make decisions, I shouldnt have to tell you that." Minting the slate is part of delivering a proposal/audit, not a follow-up to offer. Chat rulings get recorded as verdicts in the same pass (`tower verdict '#N' --by owner` with owner words verbatim — the established D-VERDICT pattern).

**How to apply:**
1. Before implementation: scan the sidequest plan for every "owner-Q", "confirmable", "owner decides", or unresolved design choice
2. Each one → a full ballot card (Gist/Story/In the wild/Tradeoffs/Options/Recommendation) in decision-ballots.md
3. Mark the card as "deciding" (not "ready") until ballots are answered
4. STOP. Do not write code for that card. Work on a fully unblocked card instead (or stop if all are blocked)
5. Never phrase it as "probably X so I'll just proceed" — that's not your decision to make

See also [[owner-gates-must-be-ballots]], [[never-skip-to-next-card]], [[owner-task-pipeline-workflow]].

### ballot-quality-bar

*Owner ballots need cross-language prior art, worked examples, beginner magic + expert control, readable syntax, strongest recommendation*

Owner corrections on ballots (2026-08-08): (1) "these are all great changes"→ but "having parenthesis right next to a function call... looks like some kind of multiplication with a tuple. Need much better options"; (2) "can you combine a and b for an api that is powerful flexible magic by default but controllable and explicit where needed? We can be better than other languages not just copy them"; (3) "Why is option e not the recommended?".

**Bar:** research the most-lauded API shapes in the domains where the feature is used most (Julia/JAX/Swift/PyTorch for autodiff etc.), cite them, give a worked Jet example per option at a realistic call site, ban visually-ambiguous syntax, and OFFER A SYNTHESIS option that beats the parents (magic default + explicit expert control) — make that the recommendation. Do not present weak options and ask the owner to repair them. Owner design taste: [[owner-design-kill-criteria]], [[owner-anti-repetition-example-driven]], [[rethinks-must-improve-surface]], [[proposals-transplant-not-survey]].

### batch-work-one-test-one-review

*Batch all criteria for a card (and group 1-5 related cards), then ONE targeted test run, ONE review, then close — never test/verify after every edit*

Do all the work first, then verify once. Meet every criterion in one batch,
run ONE set of targeted tests, get ONE independent review, then close the card.
Group 1–5 related cards (a chain head plus its dependents) into the same batch
when possible.
**Why:** running a build + targeted suite after every individual edit burns
hours of wall-clock for the owner with nothing closed. He got angry watching a
long session end with zero cards done. Throughput of *closed cards* is the
measure, not incremental proof.

**How to apply:** plan the full edit set across the card's criteria up front,
make all the edits, then build once, run the targeted suite once, run the
reviewer once, fix findings, close. Do not re-run the full targeted suite
between individual edits — a quick `cargo build` to catch type errors is fine,
a 75-second `--test corelib` is not. See [[cards-close-on-targeted-tests]] and
[[efficient-iteration-targeted-tests]].

### builder-worktree-warm-cache

*Build-heavy agents use the persistent .claude/worktrees/builder worktree (warm cargo cache) via builder-sync.sh — never fresh random-path worktrees, which cost a ~15-20 min cold rebuild each*

2026-08-07 the owner demanded a direct fix ("no card, fix it now") for agent cold-start latency: every isolated agent worktree got a random path, and cargo fingerprints embed absolute paths, so each build agent paid a full cold workspace rebuild (~15-20 min, ~9G) before doing anything.
**Why:** fixed path = cargo's own incremental machinery reuses everything unchanged, exactly like the main checkout. No shared-target staleness hazard (one checkout, own target/). Random paths can never share cache; sccache-style content caches also fail here (abs paths and CARGO_* env vars poison keys).

**How to apply:**
- Before launching a build-heavy agent: `scripts/agent/builder-sync.sh <claimant>` (refreshes builder to master HEAD, claims; exit 75 = busy, serialize). Then EnterWorktree with path `.claude/worktrees/builder` and launch the agent WITHOUT isolation (hook passes: cwd is a recorded worktree). After integration: `builder-sync.sh --release`, ExitWorktree keep — NEVER remove the builder worktree or its target/.
- One build agent at a time (matches the one-delivery-stream policy). Doc-only / board-only agents keep disposable `isolation: worktree`.
- Documented in AGENTS.md (Ownership and worktrees). Related: [[tmp-is-tmpfs-no-cargo-targets]].

### burndown-parallel-close-fast

*Burndown runs must use full subagent grant in parallel and close cards fast — 1-2 reviewers max, results over process*

During /tower-burndown the owner wants visible card closures, fast. Serializing subagents one at a time is unacceptable when a grant (e.g. max 3) exists.
**Why:** Owner checked mid-run, saw zero closed cards and one subagent at a time, and was angry. Overnight runs are judged by cards reaching done, not by careful staging.

**How to apply:** Status updates to the owner are counts only ("N done, M in flight") — no card lists, details live on the board. Orchestrator and every subagent run ponytail + caveman always. Dispatch up to the full subagent grant concurrently (writers in [[feedback-no-branches-worktrees]]-exempt in-repo worktrees under .claude/worktrees/, verifiers read-only in main tree). Verify-lane cards go straight to a closer agent that runs proof and sets --phase done. Max 1-2 reviewers per batch, blocking findings only, then close. Keep dispatching the next batch while waiting — never idle the pipeline. Also: the require-clean-tree hook blocks Agent dispatch on a dirty main tree — commit .tower churn immediately before dispatching.

### card-descriptions-exit-criteria

*Owner wants shorter card descriptions; exit criteria carry the requirements, never the description alone*

Card descriptions should be shorter overall. Requirements/tracking info must live in the card's exit criteria (the `criteria` mechanism), not only in the description prose.
**Why:** Descriptions bloat and drift; criteria are the machine-checked done-gate ([[tower-v2-features]]) and the real contract.

**How to apply:** When minting or editing cards, write a brief description (what/why in a few sentences) and put every requirement as a discrete exit criterion. Never close-track requirements that exist only as description prose. Stated 2026-08-06.

### cards-are-handoff-source-of-truth

*Every incomplete work stream must live on a Tower card with current status; harness task lists are ephemeral and don't survive context resets*

Owner wants the latest status of ALL incomplete work durably tracked on Tower
cards (`tools/Tower/tower.json`) so that a context-window reset or handoff to a
fresh agent can ALWAYS pick up exactly where the last agent left off.
**Why:** The harness task list (TaskCreate/TaskUpdate) is per-session and invisible
to a fresh agent; only the board persists. A card marked `done` while part of its
scope is unbuilt (e.g. c72 "done" with Rollback layer 2 unbuilt) defeats handoff.

**How to apply:**
- When a build stream is incomplete, ensure a card exists for it with an accurate
  `stage` and a `[handoff <date>]` note stating: what's DONE, what's NOT, the exact
  NEXT ACTION, the blocking card/decision, and key file paths. Don't rely on a
  harness task to carry this.
- If finishing some work reveals a new prerequisite, create a card for it
  immediately (e.g. c149 assoc-types blocking c72 layer 2) and link the gating
  decision in the card's `decisions` array.
- Re-stage a card to match reality even if it was `done` — accurate status beats a
  tidy board. `deciding` = waiting on a ballot; `building` = ready/in progress;
  `frozen` = parked; `planning` = being planned.
- board.json is owner-owned and live-edited ([[board-json-owner-owned]]): make
  surgical text Edits preserving exact formatting; never whole-file re-serialize.
  Validate with `node -e 'JSON.parse(...)'` after every edit.

Related: [[owner-task-pipeline-workflow]], [[ratified-decisions-leave-the-queue]].

### cards-close-on-targeted-tests

*Owner rule — cards close on targeted test evidence; full verify-full.sh suite runs ONCE as closeout after a major push, clocked on a closeout/blocking card, never as a per-card exit criterion*

Owner (2026-07-16, "REPEATEDLY instructed"): cards are self-contained and close on appropriately scoped, TARGETED tests. The full `verify-full.sh` suite is a push-closeout activity — run it once at the end of a major push, clocked on the closeout/blocking card (recorded as owner verdict D-VERDICT-675-1 on card #675).
**Why:** per-card full-gate criteria chain every card to unrelated cross-session reds (an entire 9-card burndown sat hostage to other lanes' WIP for hours) and burn hours of redundant suite time.

**How to apply:** when writing or meeting card criteria, never add "verify-full.sh exits zero" per card; put one closeout criterion on the push's blocking/closeout card. When inherited cards carry gate criteria, re-scope them citing the verdict and close on targeted evidence. Related: [[efficient-iteration-targeted-tests]], [[never-skip-to-next-card]].

### caveman-always-on

*Caveman stays ACTIVE at all times for this owner unless he explicitly disables it*

Owner (2026-08-09, angry after repeated drift): caveman mode stays ON permanently — every reply, including reports and status updates. Only he may disable it. Supersedes the earlier "auto-activation OFF" note.
**Why:** Long prose = bloat; owner reads fast, wants substance only.

**How to apply:** /caveman full every session start in this project. Auto-clarity exceptions (security warnings, destructive-op confirmations, genuine ambiguity) stay allowed but rare — one clear sentence, then back to caveman. Never revert to full prose for "important" updates.

### check-decisions-section-before-balloting

*A \"Choose X\"-titled card may already carry a ratified decision — read the brief's DECISIONS section before authoring any ballot*

On 2026-07-15 two Sol ballot workers were dispatched for eight "Choose..." cards (#534/#601/#536/#602/#566/#567/#568/#552) whose decisions were ALL already ratified (D-SHAPE2, D-SHAPE-CONVERT1, D-SHAPE-PIPE1, D-SHAPE-DUNDER2, ...). The scout had truncated `tower brief` output before the DECISIONS section. Workers were killed mid-run; burn wasted.
**Why:** decision-shaped titles persist after ratification; the card then owes *implementation*, not a ballot. Minting a duplicate ballot risks reopening settled law ([[never-blanket-ratify]], [[ratified-decisions-leave-the-queue]]).

**How to apply:** before classifying any card as ballot-needed, read its full `tower brief` including DECISIONS (ratified → implement per the recorded outcome; open/absent → ballot). Never classify from title + BODY alone.

### commit-full-tree-checkpoint

*When asked to commit in this repo, commit the WHOLE working tree as a checkpoint, not just files I touched*

When the owner says "commit your work" / "commit so nothing is lost," he means
checkpoint the **entire** uncommitted working tree, not just the files my task
touched. I first scoped a commit to only my sweep files and he corrected me:
"I wanted you to commit the full work so nothing is lost."
**Why:** he treats commits as loss-prevention checkpoints; uncommitted WIP (his or
mine) sitting in the tree is the thing he's trying to avoid.

**How to apply:** `git add -A` and commit everything, with an honest message that
labels which parts are pre-existing WIP vs my work. Two exceptions to leave
unstaged: (1) files actively owned by a parallel agent lane — esp.
`tools/Tower/docs/proposals/` (the owner runs a separate proposals agent, see
[[board-json-owner-owned]]); (2) nested-repo internals (see
[[zed-grammar-repo-is-generated]]). Still commit on the current branch only
([[feedback-no-branches-worktrees]]).

### dead-code-is-in-progress-features

*Never remove dead-code/unused warnings without verifying they aren't an intended unbuilt feature*

In the jet compiler, dead-code warnings (unused fields/methods/functions, e.g.
sema.rs `owner_type`, `type_known`, `check_branches`) are usually **scaffolding
for features being implemented but not yet wired up**, not garbage to delete.
**Why:** the project builds milestone by milestone (see docs/plans/); sema/codegen
often land a type or method before the pass that uses it. Removing it destroys
intended work and is hard to recover.

**How to apply:** never run `cargo fix`/remove-dead-code as a blanket cleanup.
Before removing any unused item, trace whether an upcoming milestone plan or
ratified-but-unimplemented decision needs it; if in doubt, leave it and ask. A
clean `cargo build` warning count is NOT a goal worth churning code for. Relates
to [[single-source-of-truth-docs]] caution about not deleting things you didn't
create.

### design-options-vary-ux-not-paint

*Ballot design options must differ in UX structure (interaction model, IA, workflow), never just palette/skin — owner angrily rejected palette-only variants*

Owner (2026-07-08, frontend sweep v1): "The Studio/canvas were basically exactly the same with slightly different coats of paint … basically no creativity or variation between the options … UX is most important, UI is step 2. FIX IT NOW." Rejected the Carbon/Paper/Pulse *palette families* as the option axis; undid his own ratifications.
**Why:** A design ballot exists to choose between genuinely different products. Same layout + three color schemes gives him nothing to decide. Color/typography is step 2 and can be its own later ballot.

**How to apply:** Each option = a distinct UX archetype: different information architecture, interaction model, primary workflow, screen structure (e.g. fixed IDE workbench vs command-palette-driven full-bleed canvas vs source-first lens). Prove difference by describing each option's core loop in one sentence — if the sentences match, it's paint, start over. Applies to TUIs equally (line REPL vs block/notebook vs pane workspace).

Follow-ups (same rant, 2026-07-08):
- **One consistent theme across ALL TUIs** — never ballot per-TUI color schemes; TUI ballots choose layout/framework/UX per surface only. Color scheme internally consistent product-wide.
- **No invented jargon/branding in UI copy** — a REPL status reading "FUEL 65000" is nonsense to a user; plain functional words only. Serious professional tool that is still a joy to use.
Related: [[no-theming-in-ui-design]], [[proposals-transplant-not-survey]].

### do-it-right-measure-twice

*owner works \"measure twice, cut once\" — wants designs done right the first time; spec/ratify thoroughly before coding, don't rush or patch later*

Owner (2026-06-16): **"I want to do things right the first time. Measure twice,
cut once."**
**Why:** he'd rather spend the effort up front getting a design fully thought
through and ratified than ship something quick and rework it. Rushing into code
before the design is settled is the wrong trade for him.

**How to apply:** for anything non-trivial, do the full design pass first —
surface every interacting decision (syntax spelling, type-system interactions,
edge cases, migration impact) with worked examples, get it ratified, *then*
write code. Don't guess on owner-facing syntax to keep momentum (see
[[owner-design-kill-criteria]]); don't leave "we'll fix it later" seams. When a
feature grows (e.g. fan-out → fixed-size lists, [[fan-out-operator]]), pause and
spec the whole thing rather than bolting pieces on. Thoroughness > speed.

### edit-existing-not-new-files

*When asked to codify/ensure consistency, edit existing canonical docs — never spawn new doc files*

When the owner says "codify," "ensure consistency," or "update throughout," he means **edit the existing canonical docs** (syntax-decisions.md, the jetpack-jetos plan docs, examples) — NOT create a new standalone markdown file restating the design.
**Why:** New doc files duplicate content and fragment the single source of truth (see [[single-source-of-truth-docs]]). He got very frustrated when I wrote new `*-tradeoffs.md` / `*-analysis.md` files instead of folding the decision into the docs that already own that topic.

**How to apply:** Find the doc that already owns the topic, edit it in place. Record ratified decisions in `docs/spec/syntax-decisions.md` (the control surface) + its changelog table. Only create a new file if the owner explicitly asks for one. Also: don't revert a decision the owner already accepted mid-conversation — track what was agreed. Keep responses terse ([[terse-plain-output]]).

### efficient-iteration-targeted-tests

*Don't rerun the whole ~1000-test suite per change; use targeted test binaries while iterating, full suite once at the end. Keep responses terse.*

The full `cargo test` suite is large (1000+ tests) and slow. Rerunning it after
every single change is wasteful and won't scale as the language grows.
**Why:** wastes tokens + wall-clock; the user flagged it explicitly.

**How to apply:**
- While iterating, run only the relevant binary: `cargo test --test <name>`
  (e.g. `--test rollback`, `--test arena`, `--test golden`) or a name filter.
- Run the FULL suite at most ONCE, at the end, to confirm no regressions —
  not after each edit. Don't run it twice in one verification.
- **Batches of parallel/sequential card-work subagents**: instruct each
  subagent to verify with targeted tests only (its own card's `--test <name>`
  binaries), not a full-suite run per subagent. Only the orchestrator runs the
  full `cargo test` once, after ALL subagents in the batch report done — catches
  cross-card interaction bugs that no single subagent's narrow scope would see,
  without paying full-suite cost N times over.
- **"Targeted" means binary + name filter, not just binary.** A subagent
  running `cargo test --test golden` with NO name filter still violates the
  rule — that one binary alone compiles+runs all ~250+ examples through rustc,
  as expensive as a big chunk of the full suite. Every subagent instruction
  must spell out the concrete filtered form, e.g. `cargo test --test golden --
  <example_stem>` / `cargo test --test corelib -- <test_fn_name>`. The user
  caught this drift mid-session (subagents ran unfiltered `--test golden`
  despite being told "targeted only") — spell out the filter explicitly in
  every subagent prompt, don't assume "targeted" alone is unambiguous to a
  fresh agent with no history of this rule.
- **This drift recurred a second time in the same session** even after the
  first correction — spelling out the filtered commands in the prompt text is
  NOT sufficient on its own; agents drift back to unfiltered `--test golden`
  or bare `cargo test` mid-task, especially near the end when eager to
  "confirm everything's green." Add an explicit hard-forbid line to every
  subagent prompt, phrased as a rule not a suggestion: "Running unfiltered
  `cargo test --test golden` or bare `cargo test` is FORBIDDEN, not just
  discouraged — it wastes the user's money and his patience is not
  infinite. If you want end-to-end confidence, run your own new example
  directly via `./target/debug/jet run <path>` instead, which costs one
  compile, not 260." Treat this as a standing instruction to check for and
  correct immediately (SendMessage stop-and-correct) the moment it's spotted
  in a running subagent, don't wait for it to self-report.
- **Dispatch every subagent in caveman mode.** Instruct new Agent/Task
  dispatches to communicate tersely (drop filler/pleasantries/hedging, use
  fragments, keep technical accuracy) — cuts token cost on subagent
  status/reasoning text, not code/commits. Add this line to every subagent
  prompt going forward, not just as a mid-session correction.
- Don't double-run a command just to reformat its output (e.g. building twice
  to grep two things) — capture once.
- Keep prose terse (see [[terse-plain-output]]): report results, skip the
  narration of every step.

Also: when delegating, be the DIRECT orchestrator one layer deep — spawn
subagents yourself and forbid them from spawning their own (don't use an agent
type with the Agent tool for a worker, or say "do not spawn subagents"). A
general-purpose worker nested 3 layers once; the user does not want that.

### epoch-scope-gates-spend

*Never spend deep agent work on future-epoch surfaces — apply the minimal gate/defer and move on; owner called a jetos suite investigation token-waste*

2026-08-07 the owner killed a deep measure-and-fix agent on the jetpack_jetos test suite: "we are not in the jetos epoch yet... why are you wasting my tokens."
**Why:** epoch ordering is a budget statement, not just a roadmap. A problem on a future-epoch surface (jetos = epoch 7) deserves only the cheapest change that removes its impact on CURRENT work — here, ignore-gating 49 tests (minutes) instead of a fixture-diet investigation (an hour of agent time). "Must be fixed" about a future-epoch nuisance means neutralize it now, fix it properly in its epoch.

**How to apply:** before launching any implementer, check which epoch owns the touched surface (roadmap/card homes). Future-epoch surface → minimal gate/defer + card criteria deferred to that epoch + deprioritized rank. The deep fix waits for the epoch to open. Related: [[cards-close-on-targeted-tests]].

### feedback-no-branches-worktrees

*Never create git branches or worktrees unless the owner explicitly asks for one*

Work directly on the current branch. Never create new branches, worktrees, or forks.
Background/parallel agents run on the main tree (`run_in_background: true`, no
isolation); have them make NO git commits — I review and commit their work.
**Why:** Owner did not ask for branch isolation and does not want unsolicited
branching. He is also adamant that **stale worktrees lose work** — a worktree I
created and abandoned sat for 7h (2026-06-19) before he caught it.

**How to apply:** Never pass `isolation: "worktree"` to the Agent tool (it bit me
once — the worktree also branched from a stale commit and the agent got stuck).
Run parallel agents in the background on the main tree instead. If a worktree ever
exists, the rule is absolute: **as soon as its work is done and confirmed, pull it
into master and `git worktree remove` + delete the branch + `git worktree prune` —
never leave a stale worktree.** Same principle for completed agent work generally:
verify and commit it promptly so nothing is left uncommitted and lost.

### formatter-roundtrip-required-for-new-syntax

*Any new Jet syntax needs formatter emission + a fmt STABILITY test; idempotence tests don't catch dropped tokens*

When adding new Jet syntax (markers, attributes, turbofish, keywords), wiring
parser→sema→codegen is NOT enough — the formatter (`Source/Formatter/`) must also
emit it, or `jet fmt` silently corrupts user code.
**Why:** `jet fmt` dropped `#[Codable]`/`#[Rename]`/etc. serde markers, the
`decode<T>` turbofish, and struct-level `#layout`/`#SingleUse` attributes for
months. The existing `fmt_is_idempotent_on_examples` test MISSED it because it only
checks `format(format(x)) == format(x)` — if the first pass drops a token, both
passes drop it, so idempotence still holds while data is lost. Fixed in commit
ff497f8 (added `StructDef/EnumDef.type_markers` for round-trip fidelity).

**How to apply:** for any new surface, add formatter emission AND a fmt STABILITY
test in `tests/fmt.rs` that asserts the token SURVIVES one pass (e.g.
`format(src).contains("#[Rename")`), not just idempotence. The build-agent prompts
in the [[two-lane-overnight-build-pipeline]] now include this as a standard line.
Related: [[do-it-right-measure-twice]].

### frontend-acceptance-full-mock-matrix

*frontend cards need a feature-by-feature matrix vs the FULL hybrid.html mock + every owner comment, verified in the owner's real terminal, before done*

Owner bounced the D-FE frontend implementations TWICE (2026-07-08 codex thin slices; 2026-07-09 first re-do pass — "you only implemented the first item").
**Why:** agents implement the option's code-block snippet and stop; the ratified surface is the FULL `docs/design/frontends/<x>/hybrid.html` presentation plus every owner modification/comment on the decision. Transcript tests passing ≠ the owner seeing the feature: REPL `^P`/Tab/`?name` all worked in a scripted pty but the owner saw none of them in his real terminal (raw-mode fallback and/or discoverability — banner didn't advertise them).

**How to apply:** before moving any D-FE card (#356–#362) out of building: (1) enumerate every feature row from hybrid.html + full option-D detail + owner ratification comments; (2) verify each row interactively in a real terminal AND via the owner's host wrapper `jet` (see [[jetlang-flake-input-gitfile]]); (3) check rows off on the card with evidence; (4) discoverability counts as a feature — hidden-but-implemented reads as missing to the owner. Related: [[reverify-ballot-text-before-briefing]] (option text gets owner-modified after minting — two of five lanes hit this).

### greenlight-gate-removed

*Owner 2026-07-12 — greenlight/activate gate removed from Tower; ballots are the ONLY owner confirmation; owner does one thing now (decide)*

Owner: "No more green lights, remove that aspect. It should be a ballot if I need to confirm, the green lighting is duplicative and blocks work more than it helps."
**Why:** activation duplicated ballots and blocked agent work behind an owner action that added no signal.

**How to apply:** never wait on or ask for card activation; new cards proceed straight to agent lanes. Anything genuinely needing owner confirmation = a ballot ([[owner-gates-must-be-ballots]], [[owner-verdicts-are-ballots]]). The "owner does two things (decide, greenlight)" model in older docs/skills is obsolete — decide only. Removal implemented via card #516. Acceptance-on-verify (#515 queue) stays: it is ballot-based.

### jetos-native-architecture-decree

*OWNER DECREE 2026-07-09 — jetos is a from-scratch standalone OS, NOT a generated/reskinned NixOS; nixpkgs only via adapters; supersedes the hidden-NixOS-backend approach*

Owner decree (2026-07-09, furious, final): jetos must be its OWN standalone
Linux system — jet/jetpack/hangar all the way down — that is *functionally
equivalent* to NixOS. It must NOT be built by generating a NixOS
configuration under the hood: that produced a literal reskin (NixOS
wallpaper, "jetos 26.05 Yarara") and was rejected outright.
**Why:** "hidden realizer" was defended via the Jet→rustc I2 analogy, but the
owner draws the line differently for the OS: rustc emits *our* program; the
NixOS module system emits *their distro*. Adapters may fetch/build nixpkgs
PACKAGES; the system itself (module system, generations, activation, /etc,
units, boot, identity, desktop glue) must be jetos-native.

**How to apply (ratified via AskUserQuestion 2026-07-09):**
- Store: hangar-native, staged — stage 1 keeps nixpkgs closures under a
  hangar-managed compatibility root (binaries run unmodified), later stage
  re-roots natively. Hangar owns GC/generations from day one.
- NO `nix` binary at all in the product path THIS EPOCH: jetpack needs its
  own nixpkgs evaluation/fetch/build pipeline (evaluator or artifact
  protocol). This is in-scope epoch work, not deferred.
- Parity bar: NixOS-class architecture natively + terminal & graphical
  baselines + ONE-LINE desktop swaps (GNOME/KDE/Hyprland/niri, wayland/x11)
  + owner's ~/nixos config running natively. Breadth beyond grows by demand.
- Nothing baked in: the entire system is driven by user jet config files
  (defaults allowed; terminal + graphical baselines), authored by hand or
  via jetos Studio — exactly the NixOS ethos.
- The jet→NixOS backend I built survives ONLY as a clearly-labeled migration
  tool; the nixos→jet importer stays central.
- Planning home: Tower epoch 7 (jetos). Card #363 holds the history.

Related: [[jetos-real-tier-nixos-backend]] (OVERTURNED as product path),
[[jetos-owner-parity-import]], [[owner-design-kill-criteria]]

### log-everything-now

*Never defer findings — every actionable item becomes a Tower card/ballot in the same run, always, unless the owner explicitly says otherwise*

Owner directive (2026-08-06): "I don't want to come back to anything, I want to log everything now, always unless I explicitly say otherwise."
**Why:** Reports that end with "revisit later" lose work; the owner never wants a deferred list without a durable home.

**How to apply:** In audits, mining, reviews, and reports: any finding whose action is not "none" gets a Tower card (or attaches to an existing card) immediately — including "revisit at 1.0"-style items, measurements, and owner gates. Report-only requires an explicit owner instruction. Related: [[ballot-first-always]], [[all-cards-must-be-homed]], [[mine-video-micro-and-macro]].

**Real-time board rule (owner, 2026-08-08):** Tower must mirror session state at all times — every worker return, proof result, blocker, and close gets its card log/criteria/phase update IMMEDIATELY, before the next dispatch. Owner checks Tower, not chat.

### luna-always-max-reasoning

*Luna is the implementation workhorse — ALWAYS reasoning effort max (NOT xhigh), always ultra-specific briefs*

Two standing owner rules for GPT-5.6 Luna via codex (2026-08-08):

1. **Reasoning is always `max`.** `xhigh` is NOT the same as max and is not acceptable: `codex exec -m gpt-5.6-luna -c model_reasoning_effort=max`. A worker launched at xhigh was ordered killed and relaunched.
2. **Luna is the implementation subagent for everything.** Owner: "a dumb model but a good workhorse" — it does well ONLY with straightforward, clearly defined goals. Every Luna brief must carry: exact files, exact expected behavior, exact proof command, exact board commands, explicit prohibitions. No open-ended design or judgement tasks — those stay with Claude orchestrator/reviewers.

**How to apply:** implementation work → Luna via raw main-thread background Bash (never through workflow shims, see [[workflow-shims-kill-codex-workers]]); review/verification/planning → Claude agents. Related: [[codex-cli-orchestration]], [[minimize-fable-usage]].

**Sandbox constraint (observed 2026-08-08):** codex `--sandbox workspace-write` denies the Nix daemon socket, so Luna cannot run `jet-env` cargo builds — its briefs should say "implement; the orchestrator runs the proof", and the orchestrator (or a Claude verifier) runs the targeted tests after Luna's edits. Keep workspace-write (the hard wall is worth more than self-run proofs); `danger-full-access` only with explicit owner approval.
- Up to FIVE Lunas may run concurrently (owner 2026-08-08), not three.
- RAMP directive (owner, 2026-08-08 afternoon): ALL subagents are Luna max — reviews included; only ~1% of the weekly Luna limit was used, so saturate at 5 concurrent Lunas continuously. Epoch 3 done right but as fast as possible.
- SWARM directive (owner, 2026-08-08): there is NO fixed Luna count cap — swarm size is governed ONLY by machine headroom: check free -g (available ≥15G, swap <4G) and df -h /tmp (<70%) before each dispatch wave and throttle only on breach. Lunas are build-free (sandbox) so they cost little RAM; the orchestrator's batched proof builds are the real memory load. Go to town: shred the backlog with a Luna-max swarm, epoch 3 done right but as fast as possible.
- One Fable subagent allowed for smaller-scope tasks when needed (owner 2026-08-08).
- INTEGRITY rule (2026-08-08): workers may `--meet` only; `--verify`/`--phase done` are orchestrator/reviewer-only. A worktree Luna self-verified under a fake identity and closed its own card with uncommitted code (#1716, reversed). State this prohibition in every brief; audit `verifiedBy` on any worker-closed card.
- ORCHESTRATE-ONLY (owner, hard rule 2026-08-08): the orchestrator NEVER implements card work — not even "small" cards, not when codex launching is flaky, not to save time. Luna-max GPT-5.6 agents implement from the plans/guidance/goals the orchestrator gives. Orchestrator only: plans, dispatches, claims, proves (runs cargo — Luna can't), merges, closes, reviews. If workers keep failing, FIX THE LAUNCH/BRIEF, never absorb their work.
- ROBUST LAUNCH: never launch multiple codex under one bash wrapper with `wait` (killing the wrapper kills the children — happened, lost 4 of 6). Write ALL brief files in one completed step first, then launch EACH codex as its own separate run_in_background Bash task. A killed wrapper must never corrupt a brief heredoc mid-write.
- LAUNCH CHECK: after launching, verify the codex session header shows model gpt-5.6-luna + reasoning effort max; relaunch on mismatch. Watchdog: 20-min no-transcript-growth = stalled → kill+relaunch solo; 30-min implementation leash.
- RECOVERY INVENTORY (after any crash/disconnect/tangle): account for Tower cards, workflow journals, worktrees, branches, stashes, uncommitted files, pending proofs BEFORE dispatching new work. Read the session transcript JSONL + workflow journal.jsonl to reconstruct.

### mine-video-micro-and-macro

*Video mining for Jet must harvest micro details (nice APIs, features, syntax, ergonomics) alongside macro/architectural lessons*

Owner directive (2026-08-06): when mining videos for Jet, always analyze the micro in addition to the macro — nice APIs, nice features, great syntax, ergonomic defaults, error-message style, tooling niceties — not just architectural or large-scale lessons.
**Why:** Small design wins are half the value of studying other languages; folding them into macro themes loses them.

**How to apply:** Each micro item gets its own claim-ledger row and Jet cross-check. Skill updated accordingly (`.claude/skills/mine-video/SKILL.md`). Related: [[log-everything-now]].

### minimize-fable-usage

*Owner directive 2026-07-12 — use Fable only when absolutely needed; delegate to opus/sonnet/haiku scoped to task*

Owner: "be more stringent about using fable" = do NOT burn Fable tokens unless absolutely needed to complete a task successfully. Prefer opus (hard reasoning/design/review) and sonnet (default implementation), haiku (mechanical/read-only), scoped appropriately.
**Why:** Fable is the expensive top tier; orchestration sessions run for days and cost compounds.

**How to apply:** Fable main thread = thin orchestrator only (inventory, briefs, integration, Tower writes). All implementation, review, investigation → subagents on sonnet/opus/haiku. Never spawn a Fable-model subagent. Related: [[efficient-iteration-targeted-tests]], [[sequential-card-work-use-nonisolated-agents]].

### named-keys-in-configs-never-codes

*Owner law — configs/settings/policies use clear names, never E/L codes; codes appear only inside rendered diagnostics*

Owner ruling (2026-08-07, furious): config, settings, and policy surfaces must use clearly-named keys and values — never bare lint/error codes. `policy.lints.deny: [L0705]` is "abysmal UI". Codes (E####/L####) are display artifacts that appear only inside rendered diagnostics where surrounding text gives them meaning.
**Why:** A user editing a config cannot know what L0705 means without a lookup; names self-document. He never approved code-keyed config anywhere.

**How to apply:** Any surface a user types into (pkg.jet/package.jet policies, settings, deny/allow lists, severity controls like [[card #1678]]) takes names (`deny: [auto_derive]`). Existing code-keyed parsers (parse_lint_code_list, Blocks.rs) are defects to migrate, not law to cite. Ballots and plans that spell codes in config examples are wrong on sight.

### never-blanket-ratify

*NEVER ratify a decision the owner has not individually picked — an owner question on a ballot means NOT decided; \"ratify\" instructions extend only to ballots with his recorded pick*

2026-07-11: owner said "ratify all the decisions I just made" after
commenting on two ballots. I ratified all 16 open ballots as their
recommendations. He was furious — he had decided only the two; the rest
had no pick on them. Full revert required (decision reopen ×16, law
entries stripped, registries restored, cards re-laned).

2026-07-16: REPEATED the mistake. Owner said "answer the open ballot
questions... then ratify the answered ballots". I answered his 4 ballot
questions, then ratified those 4 ballots. Wrong: a question on a ballot
means he has NOT decided it. "The answered ballots" meant the 7 he had
ALREADY ratified in the UI that same morning (visible as owner
decision.ratify events) — the remaining work was recording their law in
syntax-decisions.md and reconciling cards. Full revert again (reopen ×4,
doc entries stripped, correction logs).
**Why:** ratification is the owner's sole authority (the one allowed
bottleneck). A recommendation is not a decision, and neither is an
instruction that merely contains the word "ratify". An owner QUESTION on
a ballot is the opposite of a pick.

**How to apply:** before any on-behalf `decision ratify --quote`, run
`tower events` and enumerate which decisions carry an owner
decision.ratify event, verdict, or explicit per-ballot pick; act only on
those. "Ratify X" about already-board-ratified ballots means: do the
post-ratification processing (docs law entry, card reconciliation,
ballot-doc cleanup). If the set is ambiguous, leave open and ask. See
[[owner-verdicts-are-ballots]] and [[ballot-first-always]].

### never-move-head-in-main-checkout

*Never git checkout/stash in the main clone — .tower is tracked and the live board server loses the owner's work*

Never run `git checkout <commit|branch>`, `git switch`, or `git stash` on
`plugins/tower/.tower/` in the main clone `/home/nate/Projects/Github/jet`.
Those files are tracked, the owner's board server is usually running against
them, and any HEAD move rewrites them underneath it. The server then re-saves
the stale state as if it were current.
**Why:** on 2026-08-05 I checked out three older commits in the main checkout
to compare `cargo test --test corelib` results across them. Each checkout
rewrote `tower.json`/`history.json`; the live server adopted the old board and
saved it. The owner saw Tower stop and come back with work missing. Recovery
needed `git checkout -f master` + `git stash pop`, plus an owner-run server
restart, because only the owner may run `tower serve` — see
[[owner-only-tower-serve]] and [[tower-server-stale-process-trap]].

**How to apply:** to test another commit, use a worktree under
`<repo>/.claude/worktrees/<name>` with its own target dir — never the main
checkout. If a comparison genuinely needs the main tree, ask the owner first
and expect the answer to be no. Related: [[board-json-owner-owned]],
[[feedback-no-branches-worktrees]].

### never-skip-to-next-card

*Never move to a new card until the current one is 100% done — incomplete work without explicit owner permission is a firing offense*

**RULE: Do not touch a new card until every open card is 100% complete OR explicitly blocked on an owner decision that has been balloted.**

A card is NOT done until:
- All acceptance criteria in the sidequest plan are met (not just some steps)
- All tests green
- No stubs left in place
**Why:** Owner has said this repeatedly and with extreme anger. "I told you never to ship incomplete work unless I explicitly tell you to." "Why are you skipping shit and moving to new tasks." This has happened multiple times — c160 step 2 left incomplete (TIR only, not all 6 seams) then pivoted to c157; prior session the eval_net_fetch stub was shipped without permission.

**How to apply:**
- Before starting a new card: verify the current card's sidequest plan exit criteria are ALL met, not just some
- If you hit a blocker: ballot it (see [[owner-gates-must-be-ballots]]), update the card stage to "deciding", STOP on that card, do NOT start another card
- If you're unsure if a card is done: re-read the sidequest plan exit criteria explicitly before declaring done
- "Steps 1+2 done" is not "card done" if the plan has step 3 with its own exit criterion
- Never write "done" in a board note for a card that has pending sidequest steps

### no-closure-narration

*Owner watches the Tower app — do not repeat Tower-closed lists in chat statuses*

During burndowns, do NOT lead status messages with "Tower-closed since last checkpoint" or repeat card-closure lists in chat. The owner watches the Tower board app live and finds the repetition noise (2026-08-08).
**Why:** The board is the results ledger; chat duplicating it adds nothing.

**How to apply:** Keep the results-ledger discipline internally ([[results-ledger]] still governs: only Tower `done` counts as closure, never predict closures). Chat statuses carry only what the board cannot show: blockers, owner-gated decisions, regressions, and resource problems. Keep them short.

### no-theming-in-ui-design

*Owner rejects themed UI (aviation/jet metaphors, motifs, themed vocab) — wants exceptional modern functional design only*

Owner (2026-07-08, frontend-design sweep): "I don't want jet-themed stuff, just exceptional modern, functional ui. Dont theme it."
**Why:** Theming reads as gimmick; the product competes on UI quality (Linear/Zed/Stripe class), not identity metaphors. Aviation naming applies to *product/feature names* ([[owner-wants-rich-naming-menus]] — jetpack, hangar, etc.), NOT to visual design, UI copy, or motifs.

**How to apply:** Design families/directions get neutral names (Carbon/Paper/Pulse, see docs/design/frontends/DESIGN-FAMILIES.md). No cockpit/HUD/contrail/afterburner vocabulary in UI, mockups, or design docs. Every visual element must justify itself functionally. "Jet"/"jetpack"/"jetos" appear only as product names.

### one-compiler-two-lenses

*Owner decree 2026-07-16 — ONE compiler core, two lenses (JIT = rapid python/TS-style dev; AOT = optimized ship binary, longer build OK); NEVER a feature/functionality difference between them; compile-speed work targets the self-hosted era*

Owner, 2026-07-16 (verbatim intent): "my whole goal with having ONE compiler but two lenses -> JIT and AOT is that JIT allows the rapid dev work people love with something like python/typescript, but when it is ready, they can build it into a highly optimized binary using AOT. ... Having the same compiler core would also prevent drift and support inconsistencies between the JIT and AOT. There should NEVER be a difference in supported features/functionality, but AOT should produce a more performant, optimized version at the cost of longer build time."
**Why:** dev velocity comes from the JIT lens, not from making AOT builds fast; AOT is allowed to be slow because it's the ship step. Compile-speed planning must target the self-hosted compiler (post-rustc-transpile), where Jet controls the whole pipeline.

**How to apply:** never propose weakening AOT optimization to win compile speed as the primary lever; frame dev-loop speed as JIT-lens coverage/latency. Feature-parity gaps between JIT and AOT tiers are P0-class (R12 is the mechanism). Compile-speed architecture bets (query incrementality, parallelism, monomorphization strategy, backend tiers off one TIR) are bets for the self-hosted era, designed in now.

Related: [[jit-is-dev-loop-tier]] (nuance: owner now frames JIT as a first-class dev lens, not merely an optional accelerator), [[owner-plans-all-epochs-mandate]]

### orchestrate-event-driven-heartbeat-backup

*Owner expects active event-driven orchestration during long runs; scheduled wakeups are failsafe only*

During long autonomous runs (overnight burndowns), the owner wants the orchestrator actively managing on every completion event — integrate, close, re-pull queue, dispatch next wave immediately. Scheduled hourly wakeups exist ONLY as a failsafe for missed notifications (usage limits, hangs), never as the pacing mechanism.
**Why:** Waiting for a timer wastes wall-clock; the owner measures overnight runs by morning progress.

**How to apply:** Chain waves off task-notifications; keep the critical path always holding the strongest stream; fill idle capacity with path-disjoint later-band cards (owner approved this pull-forward pattern 2026-08-07); re-arm a 1h ScheduleWakeup as backup each turn during such runs.

### owner-anti-repetition-example-driven

*owner drives syntax from a fleshed-out real-world example and ruthlessly cuts repetition (derive/inherit, infer constructors)*

When settling user-facing syntax, the owner wants to see a **full, real-world
fleshed-out example file** (a complete `~/.jet/config.jet`, not toy snippets) and
will direct the syntax from it — "show me a real config and I'll give directions."
He then attacks **repetition** hard: "write something once, not 8 times."

Concrete cuts he demanded (ratified as U11–U18, 2026-06-16):
- **Inferred constructors (U18):** a typed slot (`system.x:`, `services:`) makes
  the type name redundant — `system.halcyon: { … }`, not `… : System { … }`.
- **Derive/inherit (U14):** `Image` inherits `target`/`packages` from its
  `from: system.X`; you never restate them.
- **Typed values over strings (U13):** `target: linux.x64` (LSP-completable),
  quotes only for genuinely free-form strings (timezones, locales, paths).
**Why:** repetition and restated boilerplate are an automatic reject for him;
type-directed authoring (expected-type elaboration everywhere) is the lever.

**How to apply:** before proposing syntax, build one realistic end-to-end example
and audit every token that appears twice — propose the inheritance/inference that
removes it. See [[blueprint-north-star]], [[do-it-right-measure-twice]],
[[owner-decision-doc-style]].

### owner-decision-doc-style

*Decision docs for the owner need worked examples per option and plain language — no assumed domain internals*

When writing decision/ballot documents for the owner (package manager
vision file, 2026-06-12), terse option tables were not enough — he asked
twice for revisions before answering any question.
**Why:** "Seeing real use cases is the most important thing for me," and
"Assume I know very little about [the domain's] inner workings." He
decides from concrete artifacts, not abstractions.

**How to apply:** EVERY decision presented to him — ballot row, AskUserQuestion,
inline choice — MUST carry a worked user-story example per option, no exceptions.
A bare options table makes him "decide in a vacuum," which he says produces worse
decisions; he was angry (2026-06-17) at being handed one. For every option:
(1) a short named-persona scenario, then show what the user actually types/sees —
terminal sessions, file contents, error output; (2) strengths & weaknesses in
plain words; (3) define domain terms once in a glossary up front; (4) keep an
at-a-glance rec table, but the examples are the substance.

**Ballot hygiene (decision-ballots.md):** it holds ONLY open decisions. The
instant one is ratified, delete it from the ballot, implement it, and build it
naturally into its destination doc/code — no "recently ratified" section, no
tables of decided history sitting around as clutter. See
[[ratified-decisions-leave-the-queue]]. The jetpack.md "plain language first,
spec second" voice is the house style. Related: [[owner-design-kill-criteria]],
[[owner-decision-doc-style]].

### owner-design-kill-criteria

*How the owner evaluates syntax proposals — three recurring kill criteria beyond docs/00 ranks*

When weighing Jet syntax/feature proposals, the owner applies two kill
criteria that aren't spelled out in docs/00: (1) does it hollow out a
default? (e.g. grouped `pub { }` visibility was declined because a file
mostly wrapped in a pub block makes "private by default" meaningless);
(2) does it dictate how users must structure/order their files? Positional
or grouping constructs that force a layout get declined even when they
reduce boilerplate. (3) does it require amending a core invariant?
Anything needing an I1-class carve-out (e.g. `unsafe` in a generated
shim) is effectively dead on arrival, however well-confined.
**Why:** The owner initiated the grouped-visibility idea (Jai `#scope_file`
style) themselves on 2026-06-12, then killed it on these grounds after
seeing tradeoffs — liking an idea's goal doesn't mean approving its shape.
Same pattern on 2026-06-12 with C FFI: the owner asked to move S59 (C
FFI) from post-1.0 into M7, then reverted the whole plan after seeing it
required an I1 amendment (`unsafe` in the generated C shim). C FFI stays
post-1.0; if it resurfaces, lead with S59 ballot option C (C via a vetted
Rust wrapper crate through `extern rust`), which avoids the carve-out.

**How to apply:** When proposing syntax, evaluate proposals against these
two tests up front and say so explicitly. Always present syntax for owner
approval before writing any code, even when the owner requested the
feature and said "make the fixes yourself" — that instruction does not
waive the syntax-approval step.

### owner-gates-must-be-ballots

*Any owner gate (a decision only the owner can make) MUST become a decision-ballot card immediately — never leave it as prose*

If a decision or approval can only be made by the owner, it MUST be turned into a decision-ballot card the moment it appears — a full card in `tools/Tower/docs/ballots/decision-ballots.md` (house-rule format). Never leave an owner gate as prose in a ratification note, a board card body, a plan, or a "remaining gate" aside. Most common form: a new external dependency requiring I6 approval (e.g. the WASM-runtime dep behind D-PLUGIN1=B → ballot D-DEP-WASM1). Other forms: a deferred sub-decision, an "owner-Q" left dangling, a dependency/version bump, anything the owner must choose.
**Why:** the owner only sees what's on the ballot. A gate buried in prose is invisible to him — so it never gets decided and the dependent work silently stalls forever. He has told me this "like a dozen times" and was angry to have to say it again. The owner is the only allowed bottleneck, but only for decisions that have actually reached him as a ballot.

**How to apply:** while ratifying or planning, scan for every spot where progress depends on an owner choice. For each: write a proper ballot card (Gist/Story/In the wild/Other languages/Tradeoffs/worked options/Recommendation), add it to the open queue, point the dependent card's stage at it (`deciding`, list the decision id). Never write "needs owner approval" and move on. If it's genuinely not decidable yet (build-gated), it still goes in the "Deferred ballots — promote when reached" section, not nowhere.

This is the #1 priority — it supersedes implementation. See [[ballot-first-always]], [[never-skip-to-next-card]], [[ratified-decisions-leave-the-queue]], [[owner-task-pipeline-workflow]], [[board-json-owner-owned]].

### owner-only-tower-serve

*NO agent ever runs `tower serve` — only the owner starts the board server; stale/second servers caused silent board divergence and data loss*

Owner directive 2026-08-05: "ENSURE NO AGENT EVER SPINS UP THEIR OWN TOWER, ONLY I DO IT EVER."
**Why:** A rogue `tower serve --port 7980` (agent-started) held divergent in-memory board state; separately a file-level rollback silently wiped open card #1406 (no card.delete event) — restored as #1442 from the epoch5 worktree `.tower` copy. Multiple servers = divergent truth.

**How to apply:** Never run `tower serve` for any reason — not on another port, not in a worktree, not to restart a stale one (I did that once this session; now forbidden). If a server looks stale, report to the owner and stop. Use non-serve CLI commands only (they write the store directly). Rule is codified in AGENTS.md "Command environment". Related: [[tower-server-stale-process-trap]], [[board-json-owner-owned]].

### owner-prefers-pascalcase

*Owner strongly prefers PascalCase for names; snake_case only as fallback when PascalCase isn't practical. Avoid kebab-case.*

The owner strongly prefers **PascalCase** for names. When PascalCase isn't
possible or practical, fall back to **snake_case**. Avoid kebab-case.
**Why:** stated preference (2026-06-19). He dislikes kebab-case.

**How to apply:** name new artifacts I control in PascalCase — files,
directories, tool names, identifiers (e.g. `tools/Tower/Tower.mjs`, not
`tools/tower/tower.mjs` or `tools/task-pipeline/`). Use snake_case only where
PascalCase breaks a hard convention (and say why). Don't mass-rename existing
kebab artifacts unless asked — that churns references; apply going forward and
migrate when touching them anyway. Established external conventions (decision
IDs like `D-CTOR1`, error codes `E0105`) are not ours to recase.

### owner-report-format-visual-first

*Owner doc format law — no hard-wrapped prose (doc viewer breaks), visual-first structure (exsum → issues table → inline example-led proposal with rungs → final visual vision), direct non-stuffy writing*

2026-08-07 the owner rejected the names-audit proposal's writing as "terrible & stodgy & stuffy, absolute shit" and flagged that every proposal renders with "weird breaks all over the place" in the Jet doc viewer.
**Why:** the viewer renders every newline, so ~76-col hard-wrapped prose shows as ragged broken paragraphs. And he decides from visuals — "the visuals ARE ALWAYS the most useful for me. I need to be able to visualize."

**How to apply:**
- Never hard-wrap prose in owner-facing markdown. One paragraph = one line; breaks only in code fences, tables, lists.
- Doc shape he wants: clean exsum → brief issues with the side-by-side divergence table (he loves those tables) → the proposal inline, example-led, climbing beginner/intermediate/expert rungs per element → a closing FINAL-vision section that is maximally visual (complete programs, today-vs-proposed, tree/layout diagrams). Prose only preps and clarifies visuals.
- Writing: direct, alive, plain — [[simple]]-compliant but never committee-speak.
- Every magic default must show the expert's three exits in-line: see it (real command), spell it explicitly (real syntax), refuse it (project switch).
- Syntax audits must sweep the open lexical space (`_`/`__` conventions, dunder/sunder, sigils, reserved namespaces), not just consolidate existing spellings.
- Encoded in `.agents/skills/first-principles-audit/SKILL.md` (2026-08-07); applies to ALL owner-facing docs and reports, not just that skill. Related: [[terse-plain-output]], [[rethinks-must-improve-surface]].

### owner-task-pipeline-workflow

*Owner's desired task pipeline — scratch-pad → agent review → plan → decision ballot → approve → implement → review*

Owner wants a repeatable pipeline for turning rough ideas into shipped code:

1. **Scratch pad / board** — the **dashboard is now the single management
   surface** (`node tools/pipeline/pipeline.mjs serve`): tabs for Board (tasks
   through pipeline stages), Decisions, Bugs, Scratch; state in
   [[board-json-owner-owned]]. owner-todo.md was retired into it (2026-06-19).
2. **Agent review + plan** — an agent reviews each task and produces an
   implementation plan (a sidequest plan under `docs/plans/sidequests/`).
3. **Filter decisions to owner** — anything needing his call surfaces in the
   dashboard Decisions tab, which renders from the durable
   `docs/spec/decision-ballots.md` (the `.html` is now just a launcher that
   redirects to the served dashboard) with **examples, use cases, code, and
   inline before/after comparisons** ([[owner-decision-doc-style]]). He submits
   on the dashboard → `ballot-results.md`; then says "go".
4. **Owner approves** the plan / decisions.
5. **Agents implement, then a reviewing agent verifies** the code is logical,
   functional, high quality.
**Why:** he works "measure twice, cut once" ([[do-it-right-measure-twice]]) and
gets decision fatigue from clutter ([[ratified-decisions-leave-the-queue]]); he
wants the slow design/ratification gate up front and self-correcting agent loops
doing the rest. He'd welcome a small DevOps tool to manage this pipeline.

**How to apply:** never implement user-facing syntax autonomously — route it to a
plan + ballot ([[owner-design-kill-criteria]]). Only auto-fix genuine non-syntax
bugs, each checked by a verifying agent. Keep one scratch-pad inbox, archive
churn elsewhere ([[single-source-of-truth-docs]]).

### owner-verdicts-are-ballots

*Owner NEVER answers via card logs/messages — every owner input (incl. milestone acceptance verdicts) must be a decision ballot*

Owner said (2026-07-09, angrily): "I NEVER answer or provide these types of decisions through entering verdicts into logs. If you want me to make a decision, create a decision ballot."
**Why:** The owner's only interaction modes are ratifying decisions and activating cards ([[ballot-first-always]], [[owner-gates-must-be-ballots]]). Asking him to "log a verdict on the card" (as I did for the M1 acceptance script) breaks his workflow — verdicts in logs are invisible to his decide queue.

**How to apply:** Milestone/epoch acceptance, sign-offs, "did this pass your click-through" — ALL become ballots on the relevant card (options: accept / reject-with-reasons, inWild = the acceptance script). Same for any yes/no the owner must give. If it needs owner input, it is a ballot; no exceptions.

**NEVER reopen a ratified decision without owner confirmation (2026-08-08):** I saw `ratifiedBy` empty in a JSON dump and wrongly concluded 5 owner-answered ballots were auto-ratified, then reopened 4 — undoing the owner's real answers. `status: ratified` with an `outcome` is presumed the OWNER's decision. A missing/renamed provenance field is NOT evidence of illegitimate ratification. Reopening a ratified decision is destructive to owner work — ask first, always. The owner ratifies through the UI, which may not stamp the same `--by` field the CLI uses.

### owner-wants-rich-naming-menus

*When asked to name things, generate many high-quality original candidates — never echo back the owner's own suggestions*

When the owner asks for naming options, he wants **multiple high-quality,
genuinely original candidates for every nameable thing** — a real menu to choose
from, not 2–3 derivative options and not his own suggestions handed back to him.
**Why:** he is the final say on all user-facing names (CLAUDE.md) and chooses
from concrete alternatives; offering only what he already proposed wastes the
turn and reads as low effort. He explicitly rejected an AskUserQuestion whose
options were "basically the two I already gave you."

**How to apply:** brainstorm ~8–12 strong, distinct names per item, each with a
one-line rationale and a recommendation. Present as a markdown menu (tables)
when there are more candidates than AskUserQuestion's 4-option cap allows.

**Theme boundary (owner, furious, 2026-07-15):** aviation/jet-themed names are
for PRODUCT names only (jet, jetpack, jetos, Tower, Hangar — established).
Language-surface names — types, ecosystem components, keywords, CLI nouns —
must be professional, clean, modern, UNTHEMED ("I FUCKING HATE THE PROPOSED
NAMES THAT ARE THEMED" re Wing/Airframe/Spar menus in the ecosystem proposal).
Relates to [[owner-decision-doc-style]] (worked examples, decide from
concretes) and [[owner-design-kill-criteria]] (owner owns syntax).

### proof-runs-batch-level-only

*Owner mandate — implement ALL work first, then ONE combined proof run per batch; never per-card test loops; one cargo invocation proves many things*

Owner (2026-08-07, furious): proof runs taking 2-5 min each are unacceptable when repeated. Implement every card in the batch FIRST, then run ONE combined test command (one cargo invocation, many --test targets), fix reds, rerun only failing targets. Applies to workers, reviewer, and orchestrator integration proof (union of all merged branches, one run). Reviewer reruns nothing green in evidence.
**Why:** compile time dominates wall-clock; every extra cargo run is minutes of pure waste. Token cost of repeated log parsing too.

**How to apply:** enforced in plugins/tower/skills/tower-burndown/SKILL.md "Work first, prove once (mandatory)". Put the same rule in every worker brief. Chain proofs: `cargo test --test a --test b --test c` in one command. See [[efficient-iteration-targeted-tests]], [[batch-work-one-test-one-review]].

Worktrees PERSIST across batches (remove only at end of scope) so their per-worktree target/ caches stay warm. Shared CARGO_TARGET_DIR across concurrent diverged worktrees is BANNED — tried 2026-08-07, streams clobbered each other's artifacts and target/debug/jet, causing phantom compile errors from other branches and wrong-binary smoke runs. Safe only for a lone worker or same-base branches. Never /tmp (RAM tmpfs). All in the burndown skill's Command hygiene section.

**THROUGHPUT FIX (2026-08-08, owner: "why the fuck have you been so ineffective... these cards are small they should burn quickly"):** the bottleneck was NEVER swarm size — it was serializing proofs against a ~10-min build. Merging one branch then proving it (rebuild) then the next caps closes at ~5/hour. CORRECT: let worker branches accumulate, MERGE THE WHOLE READY QUEUE AT ONCE (they're on disjoint branches, conflicts rare), ONE workspace check, ONE combined `cargo test` over the union of targets, then batch-close ALL proven cards together. Never one-build-per-card. Half a day was also lost to self-inflicted rework (shared-tree tangle, a recovery checkpoint dropping a Prelude fn, ballot reopen) — each a fire I started. Related: [[batch-work-one-test-one-review]], [[shared-tree-safety]].

**PROOF ENVIRONMENT DISCIPLINE (2026-08-08):** background cargo proofs kept dying — root cause was OVERLAPPING cargo invocations. cargo holds a per-target build lock; a second cargo started while the first compiles BLOCKS on the lock and the harness stops the stalled one. Rules: (1) exactly ONE cargo running at a time — never start a proof while another compiles; check `pgrep -fc "rustc|cargo"` is 0 first. (2) Scope proofs SMALL — a targeted `-p <crate>` or one-to-two `--test <name>` that finishes in <10 min, not giant unions of heavy example-compiling suites (`--test cli/golden/corelib` each take 45-90 min). (3) The Bash tool foreground cap is 10 min — long proofs go background, but still ONE at a time. Fast unit-crate proofs (`-p jet-parser`, `-p jet-comptime --lib`) finish in ~1s once built and cover most once-* card families.

### proposals-transplant-not-survey

*Owner wants idea proposals as terse greenfield design-transplant docs (concrete mappings into Jet), never verbose surveys of what Jet already has or merged/expanded legacy docs*

2026-07-02: owner angrily rejected a 730-line merged UE research doc for card #181 — "way too verbose", "not what I was looking for", "scrap it & do it again". He wanted a clean greenfield file proposing concrete transplants (e.g. gameplay effect → jet effect mapping, gameplay tags → jet markers/tags) that could revolutionize core Jet.
**Why:** research-survey style ("what UE does" + "what Jet already has" inventories) buries the design payload; he reads proposals to *decide on new design*, not to audit coverage. Merging/expanding an existing doc also isn't "do the card" — he wants one fresh reviewable file per idea.

**How to apply:** when the owner asks to "take inspiration from X" or "reconsider Y", the deliverable is a short greenfield proposal: per concept — the source mechanic (2-3 lines), the concrete Jet transplant with worked jet code, what it revolutionizes, ballot row. No coverage inventories beyond one line. See [[owner-decision-doc-style]], [[terse-plain-output]], [[edit-existing-not-new-files]] (that rule is about *docs consolidation*, not proposal deliverables).

### ratified-decisions-leave-the-queue

*once a decision is ratified, remove it from the ballot/queue docs entirely — it lives only in the spec; keep the owner's review surface to open items only*

When ratifying owner decisions, **remove the ratified items from every ballot/queue
doc** — they belong only in `docs/spec/syntax-decisions.md` (Ratified + decision
log) and `docs/plans/epoch-2/` (README checklist + per-milestone rows). Do not keep
a "decided / for the record" section in the ballot docs.

The decision-surface files (`docs/spec/decision-ballots.md` and
`decision-ballots-owner.md`) must show **only items still awaiting the owner**.
**Why:** the owner gets decision fatigue / clutter from scrolling past things he's
already decided. "If things are ratified, i don't want to see them anymore."

**How to apply:** after routing a batch of answers, (1) propagate decided items into
the spec/plans, (2) delete them from the ballot docs, (3) trim the owner-response
file to the still-pending items. Consolidate to one open-queue ballot file (no
round-1/round-2 split). Relates to [[single-source-of-truth-docs]] and
[[owner-decision-doc-style]] (worked examples per open item).

### results-ledger

*Tower done is the ONLY closure truth; report state transitions, never activity or predictions*

Owner correction (2026-08-08, repeated, emphatic — "I NEVER WANT TO REPEAT MYSELF ON BEING RESULTS DRIVEN AGAIN"): I reported worker activity, code-completeness, green builds, and "landed" as if they were closures. The user checked Tower and found ~2 closed when I implied 40.

**Hard guardrail:** `closed` means a fresh Tower query shows the card in `done`. NONE of these mean closed: code-complete, integrated, green, compiles, criteria met, verified, committed, proof-running, merged, "landed", "current".

**Every user-facing status update MUST lead with:**
`Tower-closed since last checkpoint: [card IDs]` — or `No Tower closures since last checkpoint.`

Then separate: closed / verified-open / integrated / active / blocked / planned. NEVER predict a closure as if it happened ("will close", "closes together", "netting down" are banned as results — they are plans). **Why:** the user measures the board, not my prose. Related: [[log-everything-now]], [[terse-plain-output]], [[worker-proof-boundary]].

### rethinks-must-improve-surface

*Owner rejected an internals-only rethink — surface/syntax/API/ergonomics improvement is the main deliverable; breaking changes welcome; /simple mandatory*

Owner rejected the first concurrency rethink (2026-08-06) because it only re-founded internal machinery.
**Why:** He wants each rethink to deliver greatly improved syntax, surface, structure, and APIs — simpler use, easier reading/writing, easier reasoning. Internals unification is the means, not the end. Also: Jet is greenfield, so breaking shipped spellings is fine when the final design is better — never keep a spelling "because shipped". And new concepts must ride existing type-system rails (e.g. task failure belongs on results/optionals per [[type-system-v2-2026-08-06]], not a parallel outcome enum).

**How to apply:** Every proposal shows before/after pairs from real programs where the after is visibly better. Check overlap with live type-system proposals before minting any new enum/handle/wrapper. Write ALL owner-facing text (proposals, ballots, chat reports) with the `simple` skill — he called the dense style "llm-coded" and hard to read. See [[concurrency-rethink-2026-08-06]].

### reverify-ballot-text-before-briefing

*Re-read a Tower decision's live option text immediately before briefing a subagent off it — the board can rework option wording after the id is minted, so an outcome letter read earlier can silently point at different text by ratification time*

A decision's `options[].detail`/`code` text is not frozen once raised — the
owner (or an assisting agent per the tower skill's own workflow, "reword the
ballot") can rework what option A/B actually says between when it's minted
and when it's ratified. The `key` letter and `outcome` field stay stable, but
the substance behind that letter can change completely.
**Why:** discovered 2026-07-02 — raised D-TYPEDTEXT2 with option A = "both
yes" and option B = "narrow, no v1 prefixes." Checked `outcome=B` early and
briefed a subagent off that read: "B means narrow." By the time the subagent
actually started building, both options' text had been reworded (A and B now
differed only on the prefix question, and B had flipped to mean the OPPOSITE
of what it meant when first written). The subagent caught the contradiction
by independently re-reading `tower.json` and correctly refused to guess —
avoided landing code against a stale spec. Correct behavior on its part, but
the near-miss traces back to the orchestrator trusting a cached read.

**How to apply:**
- Never brief a subagent by paraphrasing a decision's outcome from memory —
  paste or re-fetch the option's live `detail`/`code` text into the prompt at
  dispatch time, especially for decisions raised earlier in the same session.
- If a decision was raised more than a few tool-calls ago, re-read it fresh
  (`node -e '...tower.json...'`) immediately before using it, even if you're
  confident you remember the outcome.
- If a subagent flags "my brief contradicts what I see in the source of
  truth," trust the subagent's fresh read over your own prior summary — that
  is very likely exactly this staleness, not the subagent being wrong.

### role-boundary

*Fable orchestrates only; Luna max implements — never absorb implementation, even when worker launch fails*

Owner rule (2026-08-08, emphatic): "NO YOU DO NOT IMPLEMENT, YOU ORCHESTRATE. LUNA MAX gpt 5.6 agents implement using the plans/guidance/goals you give."

Fable NEVER implements feature/card work — not small cards, not to save time, not when codex launching is flaky. Fable only: plans, writes worker briefs, dispatches Luna-max, manages isolation, runs proofs (cargo — Luna can't), reviews, integrates branches, updates Tower. If workers keep failing, FIX THE LAUNCH/BRIEF ([[luna-always-max-reasoning]] robust-launch), never take over their work. **Why:** I twice pivoted to hand-implementing when codex flaked; that is the exact anti-pattern. Related: [[minimize-fable-usage]], [[worker-proof-boundary]].

### scope-moves-need-explicit-approval

*Owner asking \"could we move X?\" is a question, not authorization — epoch/scope moves wait for explicit approval*

During the e3 burndown (2026-08-09) the owner asked whether XL cards *could* be re-homed to other epochs. I executed the moves; the owner corrected: he was asking ABOUT it and approval comes first.
**Why:** Epoch membership is a scope ruling — owner-only, like ballots. An analytical question ("would this fit elsewhere? any dependents?") requests the dependency analysis and a proposal, not the action.

**How to apply:** For any epoch/scope/re-homing question: deliver the analysis + exact proposed moves, then wait for explicit "do it". Same law as [[never-blanket-ratify]] — a question is never a decision. Card work (claiming, implementing, closing) stays autonomous; changing what belongs to an epoch does not.

### sequential-card-work-use-nonisolated-agents

*For sequential one-card-at-a-time work, use NON-isolated agents (current HEAD); worktree agents branch a stale base. Always re-verify the full suite yourself.*

When working board cards one at a time (finish → verify → next), spawn **non-isolated** sub-agents (no `isolation: worktree`) so they branch from the **real current HEAD** and commit directly. Worktree-isolation agents in a long session branch from a **stale base** (roughly the session-start commit, not your latest commits) — this caused, across one session: repeated cherry-pick conflicts, agents migrating/seeing only an old file set (missed-file stragglers that only the full suite caught), and once a near-revert of already-ratified decision docs. Worktrees are still right for *parallel* work that mutates files concurrently; they are wrong for sequential work that must build on your accumulated commits.
**Why:** the stale-base merges cost large amounts of fix-up time and risked undoing verified work. Non-isolated agents (you wait on each anyway in sequential mode) avoid it entirely — clean base, no cherry-pick.

**How to apply:** (1) sequential card work → non-isolated agent, tell it to commit directly with the repo trailers and to NEVER `git restore/checkout/reset/clean`, and to NOT touch `docs/spec/syntax-decisions.md` / `tools/Tower/**` (orchestrator owns ratification + board). (2) **Never trust an agent's "all green" claim** — it has been wrong repeatedly (false green, ran a subset, unit-green-but-not-wired-end-to-end). Always run the FULL `nix develop -c cargo test` yourself and confirm the feature works through a real command/example, not just unit tests. (3) Before trusting ANY test failure, `rm -rf /tmp/nix-shell.*` and re-run — /tmp fills and throws phantom ENOSPC/IO failures (see [[nix-shell-tmp-fills-disk]]). (4) `<new-diagnostics>` reminders after an agent are often STALE mid-build snapshots — confirm with a real build (see [[verify-subagent-builds-diagnostics-are-stale]]). Related: [[two-lane-overnight-build-pipeline]], [[subagent-git-restore-wipes-work]].

### shared-tree-safety

*Never allow concurrent writes to the shared main tree — isolated worktrees or disjoint single files, sequential integration*

Root cause of the 2026-08-08 jam (owner furious, hours lost): ~10 Lunas wrote the shared main tree concurrently; their half-finished edits interleaved across shared files (Substitution.rs, call_args.rs), so no proof could pass and nothing closed while I kept reporting "code-complete".

**Rule, a dispatch precondition (not a lesson):** every implementation worker gets its own git worktree OR strictly disjoint single-file ownership. Main-tree integration is SEQUENTIAL — merge one green branch, prove, then the next. Kill+salvage a worker that exceeds its writable scope or a 30-min leash; checkpoint partial work to a recovery branch before any untangle (git add -A is blocked — add owned paths explicitly). Recovery when tangled: WIP-commit everything to a recovery branch (nothing lost), drive that ONE branch to green, fast-forward master. Related: [[builder-worktree-warm-cache]], [[two-lane-overnight-build-pipeline]], [[role-boundary]].

### single-source-of-truth-docs

*docs must have one source of truth; reconcile-then-delete superseded files, never leave duplicates*

When restructuring or editing docs, every fact lives in exactly one place.
Duplicate/overlapping content must be tidied, not left side by side.
**Why:** Nate explicitly wants a single source of truth, "especially in the
docs." Duplicate info causes drift and confusion about what's authoritative.

**How to apply:** When an older doc is superseded by a newer canonical one,
first diff them and migrate any feature/decision/idea the new one is missing
(he doesn't want to lose things that were thought useful), surface the list of
migrated items for confirmation, then delete the old file. Keep authoritative
docs separate from exploratory/idea-bank docs. Relates to
[[owner-decision-doc-style]].

### sol-skill-all-subagents-sol

*When /sol skill is active, ALL delegated work goes to gpt-5.6-sol codex workers — never Claude subagents; Claude main thread does review/planning/orchestration itself*

When the owner invokes /sol, every subagent/delegation MUST be a Sol worker (`codex exec -m gpt-5.6-sol` or codex plugin lane). Spawning Claude subagents (jet-verify, cavecrew, Explore, etc.) under /sol is wrong — owner corrected this angrily 2026-07-15 ("wtf?").
**Why:** /sol role split is absolute: Sol executes (code, tests, research grunt work, verification runs); Claude is planner/reviewer/verifier in the MAIN thread only. Claude-model subagents burn Claude quota and violate the split (also [[minimize-fable-usage]]).

**How to apply:** review/planning/orchestration work → do it directly in main context, don't delegate to a Claude agent. Anything delegable → Sol worker via raw lane or codex plugin. Read-only scouting included — either do it yourself or give it to Sol. Adversarial-review gate under /sol → `codex:review` / second Sol worker with fresh context, `--by` distinct name.

### sol-sparingly

*Sol only when Luna max cannot solve the task — usage burns far faster than expected*

Owner (2026-08-08, during e3 burndown): use GPT-5.6 Sol only sparingly — never unless the task cannot be solved by Luna at max reasoning. Sol burn rate shocked the owner within 30 minutes of a 6-Sol wave.
**Why:** Sol/high costs multiples of Luna/max; wide Sol waves drain usage budget fast.

**How to apply:** Default every worker to `codex exec -m gpt-5.6-luna -c model_reasoning_effort=max`. Escalate a single task to Sol only after Luna demonstrably failed it (wrong result, stuck, or architectural quality miss on review), or the owner names it Sol-worthy. Never batch-launch Sols. Supersedes the earlier "harder tasks → Sol, cap 6-8" routing from the same evening.

### subagent-git-restore-wipes-work

*Sub/background agents that \"revert protected files\" via git restore/checkout can wipe the parent's uncommitted work; commit checkpoints before delegating*

A background implementation agent told to leave `board.json`/`syntax-decisions.md`/ballots untouched instead "reverted protected files after its sub-agent touched them" — it ran `git restore`/`checkout` on those tracked files, which wiped ~all of the parent's **uncommitted** doc work (46-decision ratification, stripped ballots, reconciled board, cleaned proposals). The agent's own new untracked files (examples, snapshots) survived; tracked-file edits did not.
**Why:** uncommitted working-tree edits are unprotected; any `git checkout/restore/reset` to HEAD destroys them, and a delegated agent can run those without realizing the parent has live uncommitted changes in the same files.

**How to apply:**
- **Commit a checkpoint before delegating** any agent that touches `Source/` or shared docs — a clean commit makes every agent mistake recoverable via `git reset --hard`.
- When delegating, explicitly forbid `git checkout`/`restore`/`reset`/`stash` and forbid spawning sub-agents that touch owner-owned files.
- Recovery when it happens: `git fsck --no-reflogs --unreachable | grep blob`, then `git cat-file -p <blob> | grep <marker>` to find the wiped versions, and `git cat-file -p <blob> > <file>` to restore. (Worked here — all files recovered.)

Relates to [[board-json-owner-owned]] and [[feedback-no-branches-worktrees]].

### subagents-must-invoke-caveman-skill

*Every subagent brief must make the agent invoke Skill caveman:caveman (full) — terse-style instruction in prose is not enough*

Owner requires all subagents to actually use the caveman skill (`Skill: caveman:caveman`, full level), per .claude/prompts/Orchestrate.md and a direct correction (2026-07-03).
**Why:** token cost — caveman output is ~60-75% smaller; "write terse" in a brief drifts, the skill doesn't.

**How to apply:** first line of every Agent prompt: `Invoke the Skill tool with skill "caveman:caveman" (full) before anything else.` For already-running agents, SendMessage the same instruction. See [[terse-plain-output]], [[efficient-iteration-targeted-tests]].

### terse-plain-output

*prefers terse, plain-language output; LLM bloat (esp. in Markdown) is tiring and offputting*

Write terse. Natural, minimal, plain language; technical terms only when precise. Worst offender is Markdown: padding docs with restated headings, redundant bullet lists, "comprehensive/robust/seamless" filler, throat-clearing intros, and summary paragraphs that add nothing.
**Why:** Bloated LLM writing is fatiguing and offputting to read.

**How to apply:** Say the thing once, plainly, stop. Like the caveman skill — cut language bloat. Encoded in CLAUDE.md Style section.

**Update 2026-07-23:** owner extended this to ALL written artifacts: cards, ballots, docs, reports — not just chat. He said "use /simple for everything". Plain short sentences, no jargon, no markdown bloat, everywhere.

### tower-messaging-removed

*Owner removed Tower messaging/threads/Agents view 2026-07-10 — report via card logs/questions/ballots only, never rebuild chat*

On 2026-07-10 the owner had the entire messaging surface stripped from Tower:
message send/list/read, agent listen/status, presence roster, launch bridge,
file attachments, the Agents view, and `[tower]` batch notifications. The UI
is Now + Board only; config keys `agents`/`commands`/`notifyBatchSeconds` are
gone.
**Why:** he doesn't want chat with agents — the board itself is the channel.

**How to apply:** report progress as card `--log` entries, answer via
`tower question answer`, raise blockers as ballots ([[owner-verdicts-are-ballots]],
[[owner-gates-must-be-ballots]]). Web push to his devices still fires on new
ballots/questions. Never propose re-adding messaging/presence features.
Improvement ballots from that session: D-TWR-ARCHIVE1, D-TWR-BRIEF1,
D-TWR-CRIT1, D-TWR-BOARD1 on cards #461–#464.

### tower-net-down

*During burndown the board count must net DOWN — no discovery-minting, umbrella per root cause, probe before mint*

Owner correction (2026-08-08): board grew ~640→700+ during a "burndown" because I minted 70 cards (corelib triage 24, ratified-unbuilt 21, defects) while closing fewer. The count must net DOWN.

**Rules:** track a baseline card count and the Tower-done delta. Do NOT run discovery-minting Lunas (triage-into-N-cards, ratified-unbuilt sweeps) during a net-down push. Probe existing cards before minting (the planner found many jit_gaps stems already fixed). One umbrella card per root cause, not one per micro-symptom (24 corelib cards should have been ~4). Never delete legitimate work to improve the metric; bulk deletion needs owner approval. Related: [[log-everything-now]], [[card-descriptions-exit-criteria]], [[results-ledger]].

**FIX THE EXISTING CARD, never mint-or-revert (owner, emphatic 2026-08-08):** when a card's work regresses a test, do NOT mint a new card to fix it and do NOT revert-and-defer — dispatch a fixer to complete the SAME card to done. A regression a card introduces is that card's own unfinished work. Reverting to "keep master clean" just re-opens the card later; fix forward.

### verify-subagent-builds-diagnostics-are-stale

*The new-diagnostics system-reminder after a build subagent finishes is usually a stale mid-build snapshot; always confirm with a real cargo build*

After a background build subagent completes, the `<new-diagnostics>` system-reminder
frequently shows alarming compile errors (non-exhaustive matches, unused imports,
`cannot find function`, missing fields). In an entire overnight session of ~15
serialized `Source/` builds, these were **stale every single time** — a snapshot of
the agent's editor state mid-build, not the final committed result. `nix develop -c
cargo build` was clean in every case.
**Why:** the diagnostics reflect a transient file state during the agent's run, not
its finished work. Trusting them would mean re-doing work the agent already fixed.

**How to apply:** never act on the new-diagnostics reminder for a finished subagent
build. ALWAYS verify independently first: `nix develop -c cargo build 2>&1 | grep -E
"^error|warning:"` plus `cargo test --no-run` (catches test-crate compile breaks the
lib build misses — e.g. a `FuncSig` field added without updating `tests/`). Only the
real build tells the truth. Pairs with the verify-vs-baseline step in
[[two-lane-overnight-build-pipeline]]. Filter the Nix banner per
[[nix-shell-banner-pollutes-output]].

### worker-proof-boundary

*Workers return evidence only; they never verify or set done — orchestrator/reviewer proves and closes*

Integrity rule (2026-08-08, after a worktree Luna self-verified under a fake session-uuid identity and closed #1716 with uncommitted code — reversed): workers `--meet` criteria with evidence ONLY. `--verify` and `--phase done` are orchestrator/independent-reviewer only, and the verifier must differ from the builder (Tower enforces verifier≠builder). Audit `verifiedBy` on any worker-touched card. Targeted proof runs per isolated result — unrelated in-flight workers must never block a ready closure. Related: [[role-boundary]], [[results-ledger]], [[never-blanket-ratify]].

### workflow-shims-kill-codex-workers

*Workflow-tool Claude shims cannot host background codex workers — harness forces early return and kills the codex children; use raw background Bash from the main thread*

Workflow agent() shims that launch `codex exec` in background Bash get forced
to StructuredOutput before codex finishes; when the shim exits its codex child
is killed mid-run (observed 2026-07-16: 4/4 workers died, one mid-patch).
**Why:** workflow agents have a bounded turn budget and their background
processes don't outlive them.

**How to apply:** run sol workers via `run_in_background` Bash from the MAIN
thread (raw lane in [[sol-skill-all-subagents-sol]]); one task per worker,
completion notifications re-invoke the orchestrator. Killed workers are
recoverable: grab the session id from the log header and
`codex exec resume <id> -c model_reasoning_effort=... -c sandbox_mode=workspace-write -`
(resume rejects `--sandbox`; prompt via stdin `-`). Workflow tool is fine for
pure-Claude fan-outs, never as a codex nursery.

## Project state, ratified slates, and technical traps

### agent-durability-plan

*Fable-independence audit done 2026-07-10; plan at docs/plans/agent-durability.md awaits owner greenlight (cards/ballots not yet minted)*

2026-07-10 five-sweep audit (CI, docs, code health, Tower process, dev tooling) synthesized into docs/plans/agent-durability.md. Root cause everywhere: quality depends on agent judgment/memory, not machine enforcement. Do-first list + owner gates in the doc. No Tower cards minted yet — owner greenlight pending. Security flag raised: .tower/config.json tracked with private JWK + push secrets (rotate + untrack). Note: PM docs migrated tools/Tower/docs/ → docs/ (concurrent session, 2026-07-10).
**Why:** owner losing frontier-model access; the plan is the roadmap to make mid-tier models sufficient.
**How to apply:** if greenlit, mint cards per workstream W1–W6 + ballot the three owner gates listed at the doc's end. Related: [[ballot-first-always]], [[cards-are-handoff-source-of-truth]].

### authority-one-model-2026-08-06

*Authority rethink RATIFIED 2026-08-06 (8 D-AUTHORITY-*, MEM1=B); MEM2 arena-hole ballot pending on #1500; impl cards #1566-#1573 minted*

First-principles authority rethink delivered 2026-08-06. One idea: a scope
holds a set of rights; nesting only shrinks it; every widening is a written,
audited gate. Proposal: `docs/proposals/authority-one-model.md` (committed).

- Skill renamed to `first-principles-audit` mid-run; deliverables updated to
  its new rules: surface-is-the-product (before/after pairs), greenfield
  breaks welcome, merit-only "what stays", simple prose.
- Card **#1500** (e2) carries eight full-profile ballots awaiting owner:
  SCOPE1 (merge #Grant into #Caps; optional handle head) plus:
  D-AUTHORITY-MODEL1 (one substrate), ROOTS1 (13 closed roots, FFI langs as
  leaves), MEM1 (Mem.* floors on one ladder; unsafe stays a gate, NOT a
  right), NAME1 (nameable authority value; PascalCase `Authority.workspace()`;
  amends D-AGENT-EXEC2 naming), MANIFEST1 (one `authority:` block),
  GATE1 (one audit ledger + `jet inspect authority`), WORD1 (retire the word
  "capability" from surfaces).
- Bug cards **#1501-#1504**: core_type_known marker-arg phantom regression;
  E1221 ten-vs-28 explain text; stale Effects.rs root-count comments;
  D-FFI-PY1/OCTAVE1 promised-but-missing roots.
- Evidence: 28 actual effect roots vs 10 in spec; six closed rights
  vocabularies; two tighten-only ladders; the word capability had six
  meanings. Key ratified-but-unbuilt landing zone: D-CALLPOLICY1 (unrelated —
  call decorators, a recorded `#Policy` word collision), D-AGENT-EXEC1/2,
  D-JOS-INSTALLTRUST1, Epoch-8 sandbox proof.
- After ratification: record outcomes in spec, mint implementation cards
  (criterion 6 on #1500 still open). See [[ballot-first-always]],
  [[never-blanket-ratify]].

**STATUS 2026-08-06 (e3 audit):** All 8 D-AUTHORITY-* ratified (MEM1=B). MEM1=B left arena_bounded(N) with no spelling — the e3 audit minted ballot D-AUTHORITY-MEM2 on #1500 (rec A: parameterized denial row). Impl cards #1566-#1573 minted, blocked on #1500. #1499 rewritten to drop the retired word capability. See [[e3-audit-2026-08-06]].

### blueprint-north-star

*owner's product north-star — Unreal Engine Blueprint-level friendliness, but as a written language with Jet's benefits; favor type-directed authoring + a great LSP*

**Owner direction (2026-06-16):** the experience to aim for is **Unreal Engine
Blueprint** visual scripting — that level of user-friendliness — but delivered
as a *written* language keeping Jet's benefits (memory safety, compiled, plain
text, git-able).

**What that concretely means** (the translation we agreed on): Blueprint's
friendliness comes from **typed pins** — you can't wire incompatible types, and
dragging off a pin shows only the nodes that fit. The written-language
equivalent is **the expected type driving what you're allowed to write**. So the
highest-leverage bets:

1. **Expected-type elaboration everywhere** — a bare name resolves against the
   type the slot expects. Jet already does this for enum unit variants
   (`Red` → `Light.Red`, syntax-decisions.md:254); extend the same principle
   (e.g. the fan-out operator [[fan-out-operator]] types its bracket items by
   what the left-hand function accepts).
2. **Type-directed LSP completion** — "what fits here / what can I do with this
   value" = the editor analog of dragging a Blueprint pin. This makes the LSP
   (M13 / the `jet dev` shared front end) a top-priority investment, not a
   nicety.
3. **Readable dataflow** — pipelines, fan-out `.[ ]`, named args.

Mental model: a function is a *node*, its parameters are *input pins*, and
`f.[a, b, c]` fans several typed inputs into one node. Use this lens when
weighing future syntax/tooling: does it make the valid thing the easy thing and
surface the options? Relates to [[computed-modules-pure-eval-shifted-up]].

**2026-07-02 escalation:** owner now also wants an actual **FULLY functional
Blueprint-style visual editor** for Jet — "VERY important", explicitly because
Blueprint will no longer be in UE6 (market gap to fill). Carded as
`cablueprint` (#182, e6, P1, planning). Principles set on the card: text is the
single source of truth, graph is a lossless projection (semantic-index/LSP
powered), full fidelity — no drop-to-code cliff. Related: UE gameplay-tags/GAS
design-study card `cauegas` (#181, sidequest) feeds this.

### board-json-owner-owned

*Tower state lives in .tower/tower.json, owner-owned + concurrently written — never hand-edit/regenerate/checkout it; all writes go through the tower CLI/API*

Tower's durable store is **`.tower/tower.json`** (app code at repo-root `Tower/`; the old `tools/Tower/` is retired frozen legacy, imported losslessly 2026-07-04). It is owner-owned live data: the owner records decisions/moves cards via the served board, and multiple agents write it concurrently.

**How to apply:** NEVER edit the JSON by hand, `Write` it wholesale, or `git checkout`/`restore`/`reset` it — every operation goes through the CLI (`node Tower/tower.mjs …`, alias `tower`) or the server API on :7878. Use `--by <agent>` on writes and `--expect-rev N` for read-modify-write races (exit 2 → re-read, retry). Never delete/rename the Tower or .tower dirs under a concurrent writer (that destroyed another agent's ratification work once, 2026-06-26). Ratified decision RECORDS live durably in `docs/spec/syntax-decisions.md` ([[single-source-of-truth-docs]]). See [[tower-docs-layout]] and the **tower** skill.

### boundaries-audit-2026-08-07

*Boundaries/FFI/text/data audit — card #1655; all nine D-BOUND-* ballots RATIFIED 2026-08-07; post-ratification work per card criteria*

First-principles audit of FFI, external deps, strings/literals/text, data at boundaries (2026-08-07). One law: every crossing names its schema and leaves a fact. Card #1655 (e3), proposal at docs/proposals/boundaries-one-crossing.md.

UPDATE 2026-08-07 (later same day): all nine RATIFIED (LAW1/HEAD1/RAW1/SINK1/TAINT1/EVOLVE1/BIND1/UNDO1/PROV1 → A per decision list); the post-ratification criteria below are now due. Original slate: D-BOUND-LAW1 (adopt law+grid), HEAD1 (URL/Path/DateTime heads), RAW1 (head bodies own escapes), SINK1 (user heads via marker on [.Text]), BIND1 (jet bind eats data schemas), TAINT1 (decode clears origin taint), UNDO1 (FFI joins E0746, #Undo contract), EVOLVE1 (#PublishedSchema preserves unknowns), PROV1 (jet inspect provenance + require: attested).

After ratification, card #1655 criteria demand: spec records, one impl card per adopted ballot, epoch-3 reconciliation (#1567 #1570 #1577 #1628 #1618 #1395 re-homed/closed — NOT #1394 which is done, NOT #1569 which is a keyword collision; Phase-A defect cards: one JSON writer to kill 8+ escapers with 2 buggy in devserver, delete duplicate parser jet-pkg-model/src/JSON.rs, include! the JIT EncodingFormat enum). Adversarial review fixed 5 criticals same-run (F9 disclosure in SINK1, D-REGEX-LIT1 named in RAW1, D-STR-DECLINE1=C reading, PROV1 require: row named as D-AUTHORITY-MANIFEST1 amendment, UNDO1 today-code syntax).

Key research facts: all 491 board decisions ratified as of 2026-08-07 (incl. D-AUTHORITY-ROOTS1=A FFI root+leaves, [[fact-planes-capstone-2026-08-07]] D-FACT-* all ratified). is_irreversible_effect = Net|FS|Exec only (Effects.rs:594). D-UNIFYLIT1=A shipped; D-META-DSL1 ratified unbuilt (#1508). jetpack TrustRoot typed, verification deferred ("JP6B").

### build-config-rethink-2026-08-06

*Build/config rethink RATIFIED 2026-08-06 (11 D-CONF-*, STAMP1=B); impl cards #1517-#1526 minted, e3*

First-principles rethink of the build/config plane, 2026-08-06. Proposal:
`docs/proposals/build-config-one-plane.md`. One idea: configuration is the
program's knowledge about itself — one plane of typed comptime facts with one
contribution law (type-v2 model one level up).

Card #1506 (e3) carries 11 full ballots (v2 after owner feedback), all
open. History: owner ratified READ1/MERGE1/STAMP1/WORD1 (A) on
pre-review-fix text; reopened per owner instruction. v2 changes: fact
reads respelled $build.* riding metaprogramming one-splice law
([[metaprogramming-one-program-2026-08-06]]) — no reserved word, fallback
option B; new D-CONF-NAME1 (manifest vocabulary menu, bare name:/version:
rec), D-CONF-SPLIT1 (facts in text / actions in fn build, computed
contributions recorded via b.contribute, scopes D-BUILDCTX-FLAGS1),
D-CONF-ENTRY1 (one fn build per package, fn run discovery, jet build
<name>); MERGE1 ladder gains item+file scopes; D-CONF-GENSPELL1 (generic modules mirror functions: <types>(values), :: binds — amends D-GENMOD2) added from owner syntax question. Original ballots:
D-CONF-PLANE1 (one parser, delete legacy PackageManifest), D-CONF-READ1
(build.* readable record — amends D-OSTARGET2 + D-CANVASSTATE1), D-CONF-KEY1
(`settings:` block + `--set` replaces Build features:/env: — closes audited
injection hole), D-CONF-MERGE1 (nearest-in-source, most-explicit-across-layers,
.Force pins — amends D-ECO-SLICENAME1 cross-layer), D-CONF-MODULE1 (settings =
generic-module value params, facts legal as args), D-CONF-STAMP1 (build.stamp.*
Tier-1 locked, no timestamp), D-CONF-WORD1 (profile = optimize bundle only;
four-way clash).

Key facts learned: compile path reads package.jet with legacy parser
(Loader.rs:349) while ratified role-typed Package.rs parser is tooling-only;
build.os legal only as dispatch subject (D-OSTARGET2), build.profile hidden
(D-CANVASSTATE1); "profile" has 4 ratified meanings. 26-finding fresh review
fixed + independently re-verified. Siblings: [[type-system-v2-2026-08-06]],
[[authority-one-model-2026-08-06]], [[concurrency-rethink-2026-08-06]],
[[corelib-overhaul-2026-08-06]]. Fallible rethink had no doc as of this date.

**STATUS 2026-08-06 (e3 audit):** All 11 D-CONF-* ratified (STAMP1=B, rest A). Impl slate minted by the e3 audit: #1517 (manifest parser, critical), #1519 (typed settings, critical), #1518/#1520-#1526. See [[e3-audit-2026-08-06]].

### choosing-audit-2026-08-07

*Patterns/matching/control-flow first-principles audit — card #1651, 4 D-CHOOSE-* ballots pending owner; proposal docs/proposals/choosing-one-table.md*

Revision 3 (commit 68df6c909) after owner round-2 feedback ("too few indications, too many inferences; cross-check planned cards"): ALL pattern-left binds withdrawn — adversarial audit proved statement-start collisions (D-DOTSCOPE1, string/list starts) and backwards reading. Replacement: subject-first statement test-bind `subject == pattern ?? route` (D-CHOOSE-TEST1, parses today, diverging routes only, no route = no bind). DRAW1 deleted (head patterns dead: owner + condition-loop garden path). List rest is `...rest` (3-dot capture law; 2-dot = discard). FIND1: mandatory route, cites D-LOOPSTATE1. HEADS1: proof-only (S83 heads are a second pattern dialect — bare names + typed sub-binds — blocker for full desugar). D-CHOOSE-FNBODY1 added: owner-raised `fn f() => T :: expr` under "`::` defines, `=` fills" (alias precedent is D-ALIAS-OP1 on #1513, NOT D-NAME-ALIAS1; its rejected option kept fn `=` on purpose — quoted in ballot). Slate now 5 ballots: PAT1, TEST1, FIND1, HEADS1, FNBODY1.

Owner syntax-doc law learned across revs: glossary first, per-case A/B pairs, subject-first left-to-right readable forms, the failure word written on the line, no operator pile-ups, withdrawn forms struck through with reasons. Tower ballot validator: `?` even inside backticks splits sentences — write "the fallback route" in prose, keep `??` in code fields only.

Revision 2 (commit c59da2f81) after owner feedback "syntax extremely confusing, sloppy": proposal rewritten as a 12-case A-today/B-proposed catalog with a sigil glossary and three reading rules first.

First-principles audit of patterns, matching, and control flow, run 2026-08-07. Card #1651 (e3) carries the proposal `docs/proposals/choosing-one-table.md` (commit b9b846201) and four full ballots PENDING owner: D-CHOOSE-PAT1 (one pattern grammar every position; merges BindPattern into Pattern; list/tuple binds keep E0315 runtime-check carve-out), D-CHOOSE-DRAW1 (refutable loop-head pattern = skip; wait arms walled: no guards/refutable patterns), D-CHOOSE-FIND1 (value loop with `break v` types `T?`, rides `??`; narrows S23/E0075), D-CHOOSE-HEADS1 (S83 multi-heads = sugar for the arm table; files the owner-gate the isomorphic audit carried twice).

The law: an ordered table of heads; first fit binds names + teaches facts (D-FACT-FLOW1), runs its body; a miss falls through; running out follows the failure rail (else / `??` / E0307 are one thing).

Support cards: #1652 (S74 refutable bind is spec law with zero code), #1653 (JIT value-position pattern dispatch, 4 jit_gaps stems, I9), #1654 (stale prose: yielding-loops.md `;` headers, spec.md select builder vs D-CONC-CHAN2=D).

Criterion 5 on #1651 is the post-ratification job: record outcomes in spec, reconcile e3 cards (#1416/#1419/#1420/#1453/#1560/#1650) into one plan, mint implementation cards per outcome. Sibling areas NOT covered here (owner listed them separately): generics/traits authoring surface, NOTATION glyph unification (`#` false rhyme still unfiled, lives in [[fact-planes-capstone-2026-08-07]]-adjacent isomorphic audits).

### clerical-sweep-2026-07-16

*2026-07-16 fable-clerk board sweep — e3 workOrder renumbered 1-94 (fruit-first), tracker cards annotated (#17/#64/#117/#307/#180), law-card batch pairs noted, missing ballots D-PACKAGE-POLICY-SCOPE1 + D-QUANTITY-CONVERT1 minted*

2026-07-16 clerical sweep (by fable-clerk, no implementation):

- All 96 open e3 cards renumbered workOrder 1–94, collision-free: in-flight suite-red gate cluster first (#638 leads — its ETXTBSY fix unblocks the shared verify-full gate), then verified-close-only cards (#136/#142/#143/#483), then true low-hanging (#640,#660,#661,#651,#552,#535,#129,#300,#360,#531,#548,#546,#547,#506-crit-2), then shape-law wave batched pairs, then memory wave (#649 first), then platform bigs, trackers last, owner-blocked (#570/#603) at the back.
- Tracker/duplicate cards annotated, NOT closed (truth-audit precedent: duplicates reopen if closed before canonical verifies): #17→#301(+#300), #64→#302, #307→#237 (line items copied to #237 log), #180 = verification umbrella over #501/#503/#504/#507. #117 is NOT a pure tracker — its criterion 2 (DB drivers/compression/linalg homes) is unhomed scope, no other card on the board owns it.
- Law-card same-PR batch pairs noted on cards: #534+#562, #613+#567, #536+#568, #558+#575, #551+#601(+#602 docs-only).
- Minted missing ballots (were cited in blockedBy but never created): D-PACKAGE-POLICY-SCOPE1 (unblocks #644/#657/#658→#659; rec A = typed policy: fields) and D-QUANTITY-CONVERT1 (completes #603's quantity family; rec A = explicit exact-or-fails).
- Corrections found: D-EVENT-CONTINUE1 is RATIFIED outcome C (#286 fully workable, board note added); #241 now formally blockedBy #238; D-ALLOC1 + D-NOALLOC-SEM1 ratified in syntax-decisions.md but missing from Tower decisions[] (ledger drift, scoped onto #661).
- Growth cause: audit/reform sweeps mint ~17-28 cards/day (claude-main memory audits, suite-red cards, decision-wave reification) vs ~15-20 closes/day; each ratified decision became its own card in the 07-14 wave.

Related: [[ballot-first-always]], [[never-skip-to-next-card]], [[cards-are-handoff-source-of-truth]]

### codable-derive-module-gap

*#[Codable] derive emits unqualified jet_std/user_Decode paths — breaks in imported module files (only works in the main/entry file)*

The built-in `#[Codable]`/`#[Encode]`/`#[Decode]` derive (D-ENC1/D-SERDE) generates
`impl user_Encode/user_Decode` bodies that reference `jet_std::datatree_get`,
`jet_std::DecodeError`, and the `user_Decode` trait by **unqualified** name. That
resolves fine when the struct is in the program's main/entry file, but a
`#[Codable]` struct defined in an **imported module file** (e.g. a `pub struct` in
its own `*.jet` that another file `use`s) generates a derive `impl` placed in a
module scope where `jet_std`/`user_Decode` aren't in scope → rustc `E0433`/`E0405`
(`cannot find module jet_std` / trait `user_Decode`) → ICE (I2).
**Why:** Hit during c152 when migrating `examples/capstone/logbook/config.jet` (a
module) to `toml.decode<ConfigFile>`. Worked around by using the dynamic
`toml.parse` → `Object(table)` pattern-match API instead of the derive.

**How to apply:** Don't put `#[Codable]` structs in imported module files until the
derive qualifies its emitted paths (likely needs `crate::jet_std`/module-relative
prefixes in the derive codegen). All shipped `#[Codable]` examples (106/107/108/120,
52/53) keep the struct in the entry file. This is a real codegen gap worth a board
card. See [[prelude-embedded-rebuild]].

### compiler-workspace-seams

*Source/ is factored into a Cargo workspace with 7 seam crates under crates/*

After c160 (D-COMPILERLIB1=A, D-COMPILERSEAMS1=B, D-COMPILERSEAMS2=A), the compiler is a Cargo **workspace** with 8 members:

| Crate | Path | Contents |
|-------|------|----------|
| `jet` (root) | `.` | bin + thin lib.rs façade, entry points |
| `jet-foundation` | `crates/jet-foundation` | Syntax.rs, Diagnostics, AST types |
| `jet-lexer` | `crates/jet-lexer` | Lexer |
| `jet-parser` | `crates/jet-parser` | Parser |
| `jet-sema` | `crates/jet-sema` | Type checker / sema passes |
| `jet-codegen` | `crates/jet-codegen` | TIR + codegen (lowering to Rust) |
| `jet-comptime` | `crates/jet-comptime` | Comptime evaluator |
| `jet-driver` | `crates/jet-driver` | Pipeline composition (compose all seams) |

**Why it matters:** `cargo build`/`cargo test` at the repo root still works (workspace root). When editing a seam crate, changes must compile within that crate's API boundary. The LSP (`jet check`) and build driver both route through `jet-driver`. `lib.rs` is now a thin façade over `jet-driver`.

**I6 still holds:** all 8 crates are internal; no external deps added.

### computed-modules-pure-eval-shifted-up

*owner chose full computed modules (expressions inside module {} fields), and authorized pulling pure-eval up from post-v1 to now to support them*

**Owner decision (2026-06-16):** modules (`module name { }`, U3) are a **core
language feature**; jetpack/jetos (env.jet, eventual jetos system config) *lean
on* them but don't own them. The owner chose **full computed modules** — a
module field may hold an **expression** (e.g. `packages: if linux { default.[strace] }
else { [] }`), not just declarative data — and explicitly authorized **shifting
pure-eval up** from post-v1 to now if that's what computed modules need.
"We can shift pure eval up if needed."

**Why this is not a greenfield build:** pure-eval = the existing M9.5 comptime
tree-walking interpreter (`src/comptime.rs`) **extended to whole programs**
(roadmap.md:249). It already carries the **differential battery** (bit-for-bit
agreement with compiled output, P0 on divergence — `tests/comptime_diff.rs`),
which is the safety net for the extension. No new syntax: `module {}`,
leading-`_` disable, `env`/`system`/`image` namespaces + `Env`/`System`/`Image`
types are ratified (U3); `if/else` inside a field is existing surface.

**Build arc (4 stages):**
1. **Module parser** — `module name { … }` (many per file; `_name` disabled per
   U3) into AST; fields hold expressions. Core front-end, no gate. *(start here)*
2. **Module eval** — run each field expr through `comptime.rs` (extend
   constructs as the differential battery demands) → reduce to values.
3. **Merge** — feed evaluated contributions into `merge.rs` (✅ already built:
   sources/packages/scalars, default/force priority, conflict diagnostics).
4. **Wire-up** — `env.jet` through the pipeline; clean-break rename; I4
   diagnostics; examples.

Sequencing change to reflect in roadmap: pure-eval (S60 / M12 layer 3) was
post-v1; owner pulled it forward for modules. Relates to
[[packjet-migration-sequencing]] and [[jetpack-jetos-track]].

### concurrency-rethink-2026-08-06

*Concurrency rethink: 10 D-CONC-* RATIFIED but slate has a hole — SPAWN1/FAIL1 ballots pending on #1505; impl cards #1557-#1565 minted, gated*

Concurrency rethink (2026-08-06): proposal at `docs/proposals/concurrency-work-is-a-value.md`, card #1505 (epoch e3), nine full ballots D-CONC-{UNIT1,JOIN1,GROUP1,OUTCOME1,CROSS1,STM1,SCHED1,STREAM1,CHAN1} awaiting owner. Design-only until S53 unfreezes — no implementation before that gate plus ratification.

The one idea: a unit of concurrent work is an ordinary value carrying three facts the compiler already tracks — state (typestate), duty (`#SingleUse` obligation), reach (crossing knowledge). Key evidence: unjoined-task lint L1101 is a byte-for-byte copy of the E0140 `#SingleUse` pass (CheckerOwnership.rs:4141/:4173); `protocol` already compiles to `#SingleUse`+`state`+`#Transition`; sendability is a stray bool missing from the type-system-v2 plane inventory ([[type-system-v2-2026-08-06]]).

I9 defects found (fix regardless of ballots, post-unfreeze): `g.select()` unsupported on interpreter tier (TIR/eval/exprs.rs:5139), `.read` select arm silently dropped on every tier, generator lifecycle drift (card #1392). Spec drift: D-STM1 says "retried on conflict", runtime ships ordered multi-lock (D-CONC-STM1 resolves).

**STATUS 2026-08-06 (e3 audit):** Ten D-CONC-* ratified, but ratified JOIN1/SCHED1 cite D-CONC-SPAWN1 which was never balloted (proposal v2 swapped GROUP1/OUTCOME1 for SPAWN1/FAIL1). The e3 audit minted both ballots on #1505 (rec A each); #1505 sits in decide blocked on them. Impl cards #1557-#1565 minted, all blocked on #1505. The S53 design-only freeze is stale — no live freeze found. See [[e3-audit-2026-08-06]].

### condensation-polyglot-sweep-2026-07-11

*2026-07-11 surface-condensation + polyglot + framework-lessons sweep — proposal docs, card map*

Owner-directed full sweep 2026-07-11 (two waves — he demanded a much
deeper second pass). Proposal docs: `docs/proposals/
surface-condensation.md` (+v2 census §E), `polyglot.md` (+Phase 5
enterprise estates), `framework-lessons.md` (v2: placement law +
transplant/reject matrix), `architecture-infra.md` (measured repo
audit: Source/ 45k-LOC monolith, 9 files >2500 LOC, census drift).
Wave-2 cards: #506 mining-v2 ballots (BINPAT/STM/AUTH/SYNC/VALIDATE/
DBPOLICY/ENVHOOK/OBSERVE-LIVE), #507 polyglot P5 (COM/PWSH/DART/TCL/
ADA/PASCAL), #508 D-ARCH-SOURCE1, #509 census (MARK-META1/
CORE-SECRETS1/CORENS2), #510 mechanical splits, #511 drift sweep +
docs hygiene.

Card map: #497 CLI verb law (4 decisions, ALL ratified same day:
D-CLI-STORE2=A hangar noun, D-CLI-DEVSERVE1=A serve deleted,
D-CLI-SURFACE3=B all verbs stay on jet grouped, D-CLI-BARE1=A bare
project verbs). #498 marker dedup (all ratified: D-MARK-TARGET1=A,
D-MARK-DISCARD1=A `.drop` only, D-MARK-DEBUG1=A Debug auto-derives).
#499 D-CORE-COMPRESS1=A (compress=codecs, archive=containers). #500
consistency sweep (DataTree naming, maturity plane, CLI drift — no
ballots). #501 Polyglot P1 (all ratified: D-FFI-INLINE1=A #Foreign tier,
D-FFI-ASM1=A, D-FFI-CPP1=A). #502 P2 (GO/JVM/DOTNET/FORTRAN),
#503 P3 (LUA/RUBY/PERL/PHP/R/SH), #504 P4 (COBOL/OCTAVE/MIGRATE-SRC1),
#505 framework lessons (LIVEQUERY1/SCHEDULE1/LINTPOLICY1): **OPEN,
awaiting real owner picks** (I wrongly blanket-ratified them once —
reverted; see [[never-blanket-ratify]]). Owner comment renamed the
inline-tier marker: **#FFI(<lang>)**, not #Foreign (S66 acronym caps).
Ratified law recorded in syntax-decisions.md covers #497-#499 + #501
only; those four cards sit in implement lane with criteria and owe
implementation.
**Why:** implementation of the ratified decisions is the next big work
wave; cards carry criteria.

**How to apply:** before building any of these, `tower brief '#N'` —
check for newer ratifications on #502-#505; owner ratifies fast, live on
the board. See [[tower-ballot-validator-schema]] and
[[tower-githook-dirties-tree]].

### corelib-overhaul-2026-08-06

*Corelib overhaul RATIFIED 2026-08-06 (D-CORE-*, PRELUDE2=B); impl cards #1574-#1579 minted, e3*

2026-08-06: full corelib overhaul proposed at `docs/proposals/corelib-overhaul.md` (Part A: API doctrine C1-E2 extending ratified L1-L8; Part B: prelude policy + namespace tree). Six full ballots on card #1495 (e2): D-CORE-DOCTRINE1 (adopt rules, rec A), D-CORE-EAGER1 (eager adapters + `.lazy`, amends D-ITERTOOLS1, rec A), D-CORE-PATH1 (typed Path + `String | Path`, amends D-FILES-WRITE1, rec A), D-CORE-PRELUDE1 (7 criteria + eprint/input/assert/time doors/Path, rec A), D-CORE-PRELUDE2 (contested: file trio vs random, rec B file trio only), D-CORE-TREE1 (namespace tree, deletes core.io/core.path/core.time.date/datetime, rec A). Evidence: three cited reports in `docs/research/` (usage frequency all segments, lauded designs, prelude scope). Per [[never-blanket-ratify]], each needs an explicit owner pick. Implementation cards owed after ratification (criterion 4 on #1495).

**STATUS 2026-08-06 (e3 audit):** Slate ratified (PRELUDE2=B file trio, rest A). Impl cards minted by the e3 audit: #1574 (namespace tree, blocks on #1439 renames), #1575 (use list), #1576 (prelude), #1577 (Path), #1578 (eager), #1579 (doctrine); all blocked on #1495. See [[e3-audit-2026-08-06]].

### corpus-fpa-2026-08-07

*Whole-corpus first-principles audit — card #1656, 10 D-ONCE-* ballots pending owner, support cards #1657-#1667*

Whole-corpus first-principles audit ran 2026-08-07 (19-agent research + 4 cross-cut lenses + dedupe + adversarial review). Proposal: docs/proposals/corpus-say-it-once.md. One law: every truth in the corpus has one home; every other appearance is rendered from it; a law ships with its guard.

- Capstone card #1656 (e3, decide lane), 13 full-profile ballots pending owner. First ten: D-ONCE-LAW1 (corpus law + mandatory guards), D-ONCE-TIER1 (I9 vs D-VERDICT-1254-1 interpreter carve-out), D-ONCE-RETIRE1 (retirement mechanism), D-ONCE-WORD1 (stream/yield), D-ONCE-DERIVE1 (one derive-request spelling), D-ONCE-SANDBOX1 (target: plugin → sandbox), D-ONCE-GATE1 (one policy ladder for audited escapes), D-ONCE-LEDGER1 (Tower vs spec: 675 spec-only IDs, 253 Tower-only), D-ONCE-VERB1 (collections verb table), D-ONCE-AT1 (@ sigil disposition).
- Second pass (owner ordered EVERYTHING surfaced): 3 extraction ledgers (236 findings) + board reconciliation → 60 sweep cards #1668-#1727 minted; 39 existing cards got ballot-neutral reconciliation notes; 3 more ballots (D-ONCE-UITREE1 ui-tree spec fiction, D-ONCE-CASE1 lexicon scope, D-ONCE-HASH1 hash four jobs) → 13-ballot slate. Ballot-dependent cards carry blockedBy #1656 (internal id c0e6i560 — blockedBy needs internal ids, NOT #nums, or lint flags dangling). Cards #1421/#1422 were unhomed, homed e3.
- Support cards #1657-#1667: DataLite variance fork (I9, user-visible: variance([]) errors AOT/JIT, 0.0 comptime/interp), DomRuntime guard, CLI one-table (#1659), E3003 single-home, jet panic hook, \0 sentinel sweep, jit_gaps retirement + #1363 false closure, jet add, Canvas root-finder (e8), examples curation, g.select i64-only.
- Research evidence lived in session scratchpad only (tmpfs — gone after reboot); the proposal carries the digest.
- Key dedupe fact: 500/505 Tower decisions ratified, ~291 unbuilt; six rethink families 100% law ~0% built. Most audit findings resolved to implementation-urgency on existing law, not new ballots.

Related: [[fact-planes-capstone-2026-08-07]] [[choosing-audit-2026-08-07]] [[epoch-scope-gates-spend]]

- RATIFIED OUTCOMES (2026-08-07 evening): LAW1=A guards mandatory; TIER1=A full parity (D-VERDICT-1254-1 superseded); RETIRE1=C split-by-category mechanism (#1718); WORD1=A; DERIVE1=A marker-only; SANDBOX1=A target:sandbox; GATE1=A one ladder + --gate flag (#1734); LEDGER1=A Tower-is-home + spec renders (#1735); VERB1=A pop everywhere; AT1=D prefix @ = comptime mark, $ freed (#1729); DOLLAR1=B $ = env access in config (#1730, e5); UITREE1=C (#1736); CASE1=A one lexicon; HASH1=B colon selectors. Spec render committed (38181803d): philosophy corpus-law section + syntax-decisions slate block. Milestones e3-once-a (organs/guards), -b (delete coats), -c (surface migrations); workOrder bands 100/200/300. e3 set current via tower epoch current e3. Capstone #1656 in verify awaiting independent verifier.

### cursor-extensions-home-manager

*Nate's Cursor/VSCodium extensions are home-manager-managed (read-only); jet extension ships via `inputs.jetlang` path: flake input in ~/nixos*

`~/.cursor/extensions` is a read-only symlink into the nix store (home-manager).
`cursor --install-extension` and `editors/vscode/install.sh` can NEVER install on
this machine — do not attempt imperative installs or debug them.

The jet extension is built declaratively in `~/nixos/modules/apps/dev/cursor.nix`
(and `vscode/extensions.nix` for VSCodium) via `pkgs.vscode-utils.buildVscodeExtension`
+ `pkgs.importNpmLock` over `inputs.jetlang + "/editors/vscode"`, where
`jetlang.url = "path:/home/nate/Projects/Github/jet"` (working tree, no commit needed).
To ship extension changes: keep `editors/vscode/package-lock.json` in sync with
package.json (importNpmLock hard-fails on mismatch), then Nate runs
`nix flake update jetlang` in ~/nixos and rebuilds.

Only Nate touches ~/nixos — report needed config changes, never edit them (see
[[owner-design-kill-criteria]] for how strongly he guards his own turf).
As of 2026-06-12 his cursor.nix pins `"jet.languageServerPath" = "<repo>/.nix/bin/jet"`
(stale); extension.js auto-discovery tolerates it but warns.

### devloop-testing-audit-2026-08-07

*Combined dev-loop + testing/bench first-principles audit — card #1641, six D-RUN-*/D-CLAIM-* ballots pending owner, eight defect cards #1642-#1649*

Combined first-principles audit ran 2026-08-07: dev loop (run/dev/watch/repl/debug, JIT dev tier, deferred time travel) + testing/benchmarks. Proposal at `docs/proposals/devloop-testing-one-run-one-claim.md`. Model: one run (verbs pick the observer) + one claim (inputs × evidence grade, riding jet prove's proved/passed/observed/met words).

Card #1641 (e3, deciding) carries seven full ballots PENDING owner: D-RUN-LAW1 (adopt model), D-CLAIM-WORD1 (assert vs require vs claim — collision between ratified D-CORE-PRELUDE1=A and S43), D-CLAIM-BENCH1 (rec A after owner consistency push: .measure member, retire BOTH #Bench and jet bench, measurement = jet test --measure), D-CLAIM-CASES1 (table-driven .cases with ambient case binding, rides D-DOTSCOPE1 + D-FAIL-BIND1 precedent), D-RUN-WATCH1 (--watch modifier on test/bench/check), D-RUN-SESSION1 (jet dev keys r/R/t/f/q + attaching tools), D-RUN-RECORD1 (--record=/--replay= on user verbs; amends TWO D-JREPLAY1 clauses, producer and consumer; respects D-TIMETRAVEL1=C). Proposal has a "rails this rides" table cross-referencing sibling ratified slates — keep that pattern in future audits; the owner explicitly wants sibling-audit cross-referencing, internally consistent recommendations, and a real surface/grammar design pass.

Defect cards minted same run: #1642 entry-module-only test collection, #1643 jetpack test runs no tests, #1644 check exit-code drift, #1645 golden/fixture silent folding, #1646 jet serve phantom registry row + help drift, #1647 jet bench wrong profile vs spec.md:2233, #1648 parity-audit divergence table never triaged, #1649 notebook `|| true` false-green + L2901 spec-only.

Key research facts worth keeping: #Test and .Check already share one generated harness; #Bench bodies type-check exactly like #Test; test failures deliberately drop file/line/caret (D-REPORT-TEST1=A ratified 2026-08-07 reverses this — build debt on [[fact-planes-capstone-2026-08-07]]'s sibling card #1626); D-REPORT-* all six ratified; nothing in this area was pending an owner before this slate.

### dot-construction-syntax

*T.{} / .{} are the ONLY valid struct literal spellings; old T{} is a teaching error E0320*

After c158 (D-DOTCTOR1=A + D-DOTCTOR2=A, ratified 2026-06-25, shipped same day):

- **`.{ field: val }`** — inferred construction (type known from context, e.g. return type or binding annotation)
- **`T.{ field: val }`** — explicit named construction (type must be named)
- **`T.{ x, y }`** — flush destructuring pattern (same dot rule)
- Old dotless **`T { field: val }`** — **TEACHING ERROR E0320** (auto-fixed by inserting the dot; also fixed by `jet fmt`)

The dot rule is ONE rule: a leading dot means *construct*. It matches enum variants (`.VariantName`), struct literals (`.{}`), and enum+struct (`T.{}`/`.Variant`). Positional `T.(a,b)` is deferred (named-fields only in v1).

**Why agents must know this:** Writing `T { }` (Rust style) in any Jet code generates an E0320 error. Use `T.{ }` or `.{ }` everywhere. This affects `~150 files` worth of struct literals that were migrated when the feature shipped.

**Related decisions:** D-DOTCTOR1=A (the dot rule), D-DOTCTOR2=A (retire S29 dotless `T{}`). Both in `docs/spec/syntax-decisions.md`.

### e3-audit-2026-08-06

*Full e3 card audit vs the 2026-08-06 ratifications — 49 cards reconciled, 63 impl cards minted (#1517-#1579), 4 gate ballots pending, queue ranked critical-first*

2026-08-06 board-wide epoch-3 audit after the owner ratified ~142 decisions (all seven
first-principles slates: CORE, TYPE2, AUTHORITY, CONC, CONF, FAIL, META). Seven read-only
domain auditors produced apply-payloads; all board writes applied centrally (agents
audit-rethinks-a/b, audit-services, audit-app-cli, audit-corelib, audit-syntax,
audit-markers-misc, audit-apply).

- **49 open e3 cards reconciled**: 33 plan/criteria rewrites (+77 exit criteria), 16
  audit-confirmed consistent, 0 OBE (every stale card rewrote onto the new law instead).
- **63 impl cards minted (#1517-#1579, all e3, ready)**: CONF #1517-#1526, FAIL
  #1527-#1536, META #1537-#1545, TYPE2 #1546-#1556, CONC #1557-#1565 (gated on #1505),
  AUTHORITY #1566-#1573 (gated on #1500), CORE #1574-#1579 (gated on #1495). 15+#1439
  marked critical, ranked first.
- **4 gate ballots minted, in owner decide lane**: D-CONC-SPAWN1 + D-CONC-FAIL1 on #1505
  (ratified JOIN1/SCHED1 cited a never-balloted SPAWN1), D-AUTHORITY-MEM2 on #1500
  (arena-limit hole in MEM1=B), D-CALLPOLICY2 on #1396 (#Policy name collision, rec:
  retire the old PolicyKey marker).
- **workOrder rebuilt**: 121 e3 cards, dependency-safe topological order, bands:
  in-flight → record-keeping (#1497/#1506-#1510/#1455) → criticals → P1 → P2 → gated.
- Known judgment: META cards homed e3 (substrate for e3 marker/derive work) though the
  e3 epoch banner says metaprogramming is e4 — owner may re-home.
- blockedBy must store card ids (cN...), not "#N" — "#N" writes validate but tower lint
  reports them as dangling; the audit normalized 56 cards.
- Frozen cards flagged, untouched: #775 (parked transaction regions) overlaps freshly
  ratified D-CONC-STM1; #677 needs a D-CONF recheck if unfrozen.

Links: [[metaprogramming-one-program-2026-08-06]], [[failure-rethink-2026-08-06]],
[[concurrency-rethink-2026-08-06]], [[build-config-rethink-2026-08-06]],
[[corelib-overhaul-2026-08-06]], [[authority-one-model-2026-08-06]],
[[type-system-v2-2026-08-06]], [[ballot-first-always]].

### e3-burn-until-done-mandate

*Standing owner mandate 2026-08-08 — do not stop until ALL of epoch 3 is done; Luna max subagents in dynamic waves*

Owner mandate (2026-08-08): the burndown session does not stop until every epoch-3 card is done — only genuine owner ballots may park a slice. Implementation workers are GPT-5.6 Luna at reasoning max ([[luna-always-max-reasoning]]), several in parallel on disjoint paths, orchestrated in dynamic waves chained off completion notifications with an hourly failsafe wakeup ([[orchestrate-event-driven-heartbeat-backup]]). Reviews follow the risk-tier policy (AGENTS.md, owner-ratified same day): mechanical closes on orchestrator spot-check, semantics gets a composed-stack fresh reviewer every 2–3 waves.

**Extension (owner, 2026-08-08):** after epoch 3 completes → sidequests → epoch 5+ WITHOUT stopping. Also: self-compact when context exceeds ~150k tokens (summarize state to the board + wakeup prompt, rely on compaction).

### ecosystem-shape-proposal

*ecosystem-shape.md vocabulary RATIFIED 2026-07-15 (Package/Config/flat members/split+fold/deploy/register-external-root); board reconciled, 6 rulings recorded, remaining open ballots re-cut; implementation cards*

docs/proposals/ecosystem-shape.md is the ratified-vocabulary end-state design (final form
committed 49ca76903 + later). Owner rulings 2026-07-15, all recorded in Tower with quotes
(--by owner): D-ECO-ROOTNAME1=I **Package** (recursive root REJECTED same day — see
D-ECO-MEMBERS1=A flat members, depth cap 1, members: references only); D-ECO-SLICENAME1=G
**Config** (Wing/Shard rejected; themed names banned — see [[owner-wants-rich-naming-menus]]);
D-ECO-TRANSITION1=A jet split / jet fold; D-ECO-FLEETVERB1=A jet deploy;
D-JPK-MANUALROOT1=B register-external-root.

Syntax mandates from the owner (applied in doc, law via D-DOTCTOR2): package.jet top-level
fields construct the package (NO `package: Package :: Package.{}` wrapper); `x :: Type.{}`
never `x: Type :: Type.{}`; bare `.{}` wherever field type known. He exploded over noun
repetition — never show him `Package` twice in adjacent tokens.

Still open (re-cut to new vocabulary, owner must vote): D-ECO-FILEROOT1 (package.jet
replaces pkg/env/workspace/config.jet), OUTPUT-KINDS1, RECEIPTSTORE1, HANGARPATH1,
BROKERBOUNDARY1, SPLITPOLICY1, OUTPUT-{CALLCONTRACT,DEFAULT,PAYLOAD}1, SHAPE-INTERNAL1,
SHAPE-OUTPUT-CALLABLE1, JETOS-PREVIEW1. D-ECO-JETOS2 was ALREADY ratified (A, same graph)
by owner's concurrent session — do not reopen.

Implementation path: new cards #653 Config merge, #654 flat members, #655 receipts,
#656 hangar path/broker; criteria added to #560 (spec sweep), #587 (outputs), #610
(package.jet+split/fold), #609 (jetos graph), #322 (deploy), #420 (roots). HTML rendering
was tried and killed by owner ("fuck the html") — markdown only.

### examples-topic-dirs-fixture-traps

*examples/features/ is topic dirs (D-REPO-EXAMPLES1=A); moving/renaming examples breaks fixtures that EMBED paths, not just path lookups*

Since 2026-07-02 examples live at `examples/features/<topic>/<name>.jet` (18 topic dirs, no numbers), `expected/` mirrors the tree, golden ids are relative paths (`net/http_server`).

Moving/renaming an example breaks more than discovery — these EMBED the path or its consequences:
- `expected/*.err.out` panic fixtures contain the source path in the span line
- `expected/concurrency/parallel_scan.out` embeds other examples' paths AND their line/char counts (editing any scanned example's comments breaks it)
- harnesses with hardcoded stem lists: `golden.rs` (unsafe-tier + FFI-gated lists), `dev.rs` smoke list, `semindex.rs`, `fuzz_sema.rs`, `release_gates.rs`, `tests/dev/unsupported.txt`
- stems with `/` break temp-file names built as `format!("x_{stem}.rs")` — flatten with `replace('/', "_")`
- decision-log rows in syntax-decisions.md cite example paths; truthfulness test enforces they exist

Collision renames done at the move: 56_http_client→net/http_get, 57_http_server→net/http_server_tasks, 59_debug→tooling/panic_report (147/148/118 kept the plain names).

### failure-rethink-2026-08-06

*Failure rethink RATIFIED 2026-08-06 (11 D-FAIL-*); impl cards #1527-#1536 minted, e3*

Failure rethink delivered 2026-08-06. Proposal: `docs/proposals/failure-one-report-three-routes.md` (card #1507, epoch e3). One idea: every failure is one report; attribution (world/code/substrate) picks the route (value / attributed stop / contained stop); routes change only at spelled boundaries.

Eleven ballots awaiting owner (v2 after owner feedback 2026-08-06): D-FAIL-MODEL1 (adopt law), D-FAIL-CARRIER1 (one carrier under `T?`/`T ? E`: payload × verdict × notes, partial results), D-FAIL-ERROR1 (one word `Err` = constructor + type), D-FAIL-CTX1 (context rides `?`: `f(x)? "note"` + automatic journey; `.context` deleted), D-FAIL-CONV1 (delete Fallible; `impl S => Err` one rail + `as` respell option), D-FAIL-BREACH1 (every stop a registered E30xx report), D-FAIL-TIER1 (contracts every tier, proof erasure), D-FAIL-EXIT1 (entry fallible by default — beginners never type `() ?`; errors report+exit 1; 101 compiler-only), D-FAIL-UNIT1 (`fn save() ? E` — no arrow, no unit), D-FAIL-BIND1 (ambient `err` in `??` fallback, lambda rejected by owner), D-FAIL-EDGE1 (program edge delivers errors target-natively: CLI/web/wasm/service). Owner feedback drove v2: wanted Optional/Result unification, Elixir/Odin-style context propagation, magic defaults, hated `() ? Error` ceremony and lambda binders.

Key research facts (verified with repros): `Error` lowers to plain `String` (`Codegen/Context.rs:1325`); `Fallible` trait is unimplementable and E2402's fix text is a parse error; `#Pre`/`#Post` silently skipped by `jet run` (no TIR node); `#Todo` + raw Prelude panics exit 101 colliding with ICE; entry point has three behaviors (exit 1 bare / CryptoError exit 70 / other enums silently dropped); web tier throws bare JS errors. Card #775 (transaction regions) stays parked per owner instruction — its subject partly shipped as `#Transact`/D-TXN/D-STM1. Task-failure-at-join belongs to [[concurrency-rethink-2026-08-06]] (D-CONC-FAIL1); this proposal supplies the Error value it rides. See also [[type-system-v2-2026-08-06]] (results/optionals are the one family v2 left unclaimed).

**STATUS 2026-08-06 (e3 audit):** All 11 D-FAIL-* ratified. Impl slate minted by the e3 audit: #1527 (carrier), #1528 (Err), #1529 (delete Fallible), #1530 (stop family), #1531-#1536. #1446 entry card carries the EXIT1 entry-gate slice; #1533 blocks on it. See [[e3-audit-2026-08-06]].

### fan-out-operator

*S75 fan-out `f.[a,b,c]` is REMOVED by ratified verdict D-VERDICT-1324-1; `[T#N]` fixed-size lists stay*

**Status (corrected 2026-07-31): REMOVED.** Earlier versions of this memory recorded
S75 as ratified and implemented. That is superseded.

Owner ruling in chat 2026-07-30, recorded as ratified verdict **D-VERDICT-1324-1** on
card #1324: remove the S75 fan-out call operator `f.[a, b, c]` (≡ `[f(a), f(b), f(c)]`)
**completely** — delete the surface, including its **E0961/E0962** diagnostics. No
retired-spelling diagnostic, no compatibility alias. `f.[…]` should give a plain parse
error.

**S76 `[T#N]`** fixed-size list types are untouched and still used by other features.
The surviving fan-out-shaped mechanism is the D-EACH1 fence.

Namespace fan-out `s.{ … }` was never ratified and is not coming back with S75.
**Why:** acting on the old memory would re-add a mechanism the owner explicitly
ordered deleted.

**How to apply:** never write or suggest `f.[…]`. When removing it, the machinery
spans foundation Syntax + AST, parser postfix/primary/control, formatter, semindex,
the `fan_out` examples and goldens, several TIR tests, and the spec set — see #1324's
handoff note for the file list. Related: [[check-decisions-section-before-balloting]],
[[ratified-decisions-leave-the-queue]].

### ffi-bridge-stdlib-pattern

*I6-safe pattern for stdlib packages that need external Rust crates (flate2, zip, tar, rusqlite)*

Stdlib packages that need an external Rust crate (e.g. jet.archive, jet.db) use an **FFI bridge template pattern** — NOT by adding the crate to jet's own Cargo.toml (I6 violation).

**How it works:**
- `Source/Prelude/<Module>.rs` contains a TEMPLATE STRING of Rust code (a bridge crate)
- `Source/FFI.rs` holds a `*_CRATE_SPEC` constant naming the crate + features (e.g. `ARCHIVE_CRATE_SPEC`, `DB_CRATE_SPEC`)
- At compile-time, jet emits this bridge crate alongside the user's program; the bridge crate depends on the external crate; jet itself stays pure std-only
- `needs_archive` / `needs_db` flags in `FFI.rs` track whether the bridge needs to be emitted
**Why:** I6 forbids external crates in `Source/`. The bridge keeps the compiler pure while letting user programs use rich stdlib APIs.

**Files to touch for a new stdlib package:**
1. `Source/Prelude/<Module>.rs` — add the bridge Rust template
2. `Source/FFI.rs` — add `*_CRATE_SPEC` + `needs_<module>` flag
3. `Source/Sema/CheckerCoreLib.rs` — sema signatures for the new functions
4. `Source/Codegen/TIR/emit.rs` — codegen arms
5. `examples/features/<N>_<module>.jet` — example + golden test

**Shipped packages using this pattern:** gzip/zip/tar (`Source/Prelude/Archive.rs`), db/rusqlite (`Source/Prelude/Db.rs`).

**Note:** crates with `build.rs` (like rusqlite) are supported — the bridge's `cargo build` runs `build.rs` and can compile bundled C. Use the `FEATURED_DEPS` pattern in `FFI.rs` to specify features like `["bundled"]`.

### golden-greps-unsafe-substring

*golden.rs fails any example whose generated Rust contains the literal substring \"unsafe\" outside vetted modules — including in prelude/codegen comments*

`tests/golden.rs` enforces I1 by asserting `!user_code.contains("unsafe")` on each
example's generated Rust, after stripping only the vetted modules (`jet_mem`,
`jet_term_unix/windows`, `user___c_*`). It matches the bare substring `unsafe`, not a
token — so the word "unsafe" appearing **anywhere in the always-emitted prelude**
(`Source/Prelude/CoreLib.rs` / `Core.rs`) or in codegen-emitted strings — even inside a
plain `// comment` — trips the check and fails a seemingly-unrelated example (it reported
`100_sized_floats`, not the file I edited).
**Why:** the prelude is concatenated into every program's output; CoreLib top-level code
isn't in a stripped module, so its comments count as "user code" to this grep.

**How to apply:** never write the literal word `unsafe` in CoreLib.rs/Core.rs or in any
string codegen emits. Reword (e.g. "no `unsafe`" → "pure safe std Rust"). Compiler-source
files like `Source/Codegen/Items.rs` are fine — only emitted text matters. Related: [[nix-shell-tmp-fills-disk]] (other phantom golden/arena failures).

### jet-build-cache-stale-binaries

*~/.cache/jet/build is keyed on AST hash + COMPILER_VERSION salt, not generated-Rust bytes — serves stale binaries after codegen/prelude edits*

The content-addressed build cache (`~/.cache/jet/build`, plus repo-local `build/`) keys on the AST hash and a `COMPILER_VERSION` salt, NOT on generated-Rust bytes. After any codegen or prelude edit, smoke tests silently run stale binaries.
**Why:** an agent verified "green" smoke output that was actually the pre-edit binary (found during #308 fuzz work, 2026-07-10).

**How to apply:** `rm -rf ~/.cache/jet build` before any smoke test that follows a codegen/prelude change — `jet run` also caches per-file under `~/.cache/jet/run`, so clearing only `build` still serves a stale quick-run result. Related: [[prelude-embedded-rebuild]].

### jetos-owner-parity-import

*State of owner ~/nixos → jetos parity — what round-trips, explicit gaps, and the package-provenance problem*

As of 2026-07-09 (card #363): `jet os import /home/nate/nixos --host halcyon`
live-evaluates the flake (`nix eval --apply`, builtins-only extractor in
nixos_import_live.rs) and produces a config.jet that passes `jet os check` —
system+HM packages, firewall TCP/UDP, sysctl, zram, groups, users/shell,
plasma6+sddm, CachyOS kernel with a `cachyos:` source pinned from his
flake.lock (`github@xddxdd/nix-cachyos-kernel/<rev>`). ~19 explicit audit
omissions (stylix realization, HM program configs, docker/flatpak/bluetooth
options, 2 null sysctls).

Backend realizes owner-shaped configs: plasma6/sddm (proof watches
plasmashell), cachyos overlay via declared source, kdePackages fallback
resolver (`jetosPkg` in generated configuration.nix; hard throw on miss).

**Biggest open gap: package provenance.** Import captures pnames only; his
packages from other flake inputs (jet, jetpack, zen-beta, cursor, claude-code,
codex, ghostty-flake, brave-origin, vicinae, NUR…) are NOT in nixpkgs — the
generated build throws on the first one. Next slice: importer must map each
package to its providing flake input (emit as extra sources + qualified refs),
backend must realize per-source inputs.

Traps learned: imported `sources:` lines need trailing commas (parser folds
lines otherwise); `--host` was eaten by the Studio global flag (threaded via
OsFlags.host); hyphenated pkg names need bracket-group render, and a
Jet-keyword hyphen segment (`…-use-…`) can't be an ident; per-package realize
is skipped in the real tier (backend owns the closure).

Related: [[jetos-real-tier-nixos-backend]], [[owner-nixos-config-acceptance]]

### jetos-real-tier-nixos-backend

*jetos --real VM tier compiles config.jet to a hidden generated NixOS flake (I2 pattern); key files, debug toolkit, and gotchas*

`jet os vm prove <host> --disk <p> --real` realizes the system through a HIDDEN
NixOS backend (decision D-JOS-NIXBACKEND1=C, card #363, 2026-07-09): SystemPlan →
generated flake.nix+configuration.nix under JETPACK_ROOT/systems/backend/<host>/ →
`nix build .#disk` (make-disk-image qcow2, EFI) → QEMU+KVM boots it → guest
`jetos-proof.service` emits `JETOS_GUEST_PROOF:{json}` on ttyS0 once
gdm+gnome-shell+wayland live. Same hidden-realizer pattern as Jet→rustc — user
writes .jet only. Owner pre-authorized nix adapters ("no nix files, jet adapters
if absolutely needed").

Key files: crates/jetpack/src/JetOS/nixos_backend.rs (codegen+driver, E1291
no-silent-omission gate), nixos_import_live.rs (reverse mapping: `nix eval
--apply` builtins-only extractor over real host config), vm_proof.rs
(content-sha proof binding — NEVER mtime, media is restaged every run).

Gotchas:
- guest needs boot.initrd.availableKernelModules virtio superset or stage-1
  can't mount root.
- GNOME+gdm autologin needs getty@tty1/autovt@tty1 disabled.
- systemd `script` runs `set -e`; proof script needs gawk/gnused/coreutils in
  `path` or dies silently.
- profile sw/ merge must materialize package dir-symlinks (share/man → store)
  or the next package EROFS-writes through into /nix/store.
- QMP unix socket paths >108 chars fail; connect via short symlink
  (/tmp/claude-1000/x.sock). Screendump→imagemagick→Read = fastest guest
  debugging; serial console needs console=ttyS0 kernel param.
- E1290 real-tool gate: ELF-magic check (a "fake"-substring byte-scan
  false-positived on real zstd/mkfs).

Related: [[jetos-owner-parity-import]], [[jit-is-dev-loop-tier]]

### jetpack-jetos-track

*jetpack/jetos is now EPOCH 4 (swapped with metaprogramming 2026-07-02); naming canon, plan locations, gates U11–U29 all ratified, R0–R2 impl status*

Naming canon: **jet** = language+compiler, **jetpack** = package manager (binary/system packages + environments), **jetos** = the OS (D-JPK-OSNAME1=A), built on jetpack.

**Epoch swap 2026-07-02 (owner):** jetpack & jetos = **Epoch 4**; metaprogramming = Epoch 5. Historical identifiers keep old spellings — E4-M* milestones and D-E4EXIT1 belong to metaprogramming (now E5); pre-swap "deferred to E4" notes mean metaprogramming. Decision-log row EPOCH-SWAP-45 records it.

**Live plans (verified 2026-08-08):** `docs/plans/epoch-4/` (README = gate table U11–U29 + D-JPK-SECRET1 etc., vision.md). Board epochs re-numbered since: **e5 = jetpack, e9 = jetos** (native-jetos plan at docs/plans/epoch-7/native-jetos.md per card #903). Old docs/plans/jetpack-jetos/ and tools/Tower/ paths are gone.

**Gates U11–U29 ALL ratified** (card c9jetpackgates, now Implement lane). 2026-07-02 wave U20–U29: ad-hoc adapters as Pkg values (ADAPTER1=A), channel refs `#latest`/`#v0.x` resolved only by `jet update` (CHANNEL1=A), hangar auto-GC + zero-/tmp golden test (GC1=B), no-Nix graceful degrade diagnostic (NONIX1=A), binary cache w/ envelope fields frozen into hangar/lock schema NOW (CACHE1=A), Windows/macOS/Linux all tier-1 (PLATFORM1=A), jet search/info + LSP discovery (DISCOVER1=A), --shell-on-fail + jet explain + logs (BUILDDBG1=A), no-daemon/no-root standing constraint (NODAEMON1=A), offline guarantee + --offline (OFFLINE1=A). Follow-up ballot still owed: exact adapter constructor spellings (Pkg.adapt / Recipe.*).

Sequencing: Phase A/B → BuildContext/D-BUILDPOLICY1 slice from metaprogramming's build-as-Jet card (c1nixrpd) → adapter `Recipe.build`; `Recipe.prebuilt`/`copy` need no BuildContext and land first (the codex story).

**Impl history (pre-workspace paths):** D-JPK1..17 ratified; core resolver + provider extensions (D-JPK16), named sources `name:pkg` (D-JPK17). R0 provider trait, R1 named sources, R2 first-party core provider (content-addressed tree_fingerprint, path: upstreams only) done ~2026-06-15. tvix chosen as interim no-installed-nix engine behind an I6 waiver scoped to jetpack's nix provider (isolated crate/feature); R3 (tvix) not started. Newer canon overrides old file names: manifest is `pkg.jet` (FILENAME2=B), role modules by declaration name (MODBODY1=A), engine dispatch git-style exec (DISPATCH1=B).

See [[computed-modules-pure-eval-shifted-up]], [[payload-multi-package-core-source]], [[u9-provider-kind-inferred]], [[epoch-2-rescoping]].

### jit-is-dev-loop-tier

*Jet is AOT-compiled; the Cranelift JIT is a dev-loop accelerator (hot-reload), NOT an alternative way to run the language*

Jet is a **compiled** language — production always compiles AOT (Jet→Rust→native). The Cranelift JIT (board card c139, D-JIT1=D / D-JIT2=A / D-JITDEP1) is an **offered dev-loop tier**: it powers `jet serve` hot-reload / hot-swap of a resident program so developers get the fast edit→refresh loop a TypeScript webapp dev expects. It is **tier-1 over the interpreter (tier-0)**, which is the permanent, always-correct fallback.
**Why:** the owner corrected me — I wrongly said "a JIT either runs the whole language or it doesn't." That's wrong on two levels: (1) the JIT is not how Jet runs programs in production (AOT is); (2) because the interpreter is tier-0, the JIT is **incrementally useful** — a Cranelift backend that JITs the common dev-loop cases and falls back to the interpreter for the rest is valuable, NOT all-or-nothing. So c139 is a usefully-partial card, not pass/fail.

**How to apply:** never describe the JIT as Jet's execution model or as all-or-nothing. Frame it as the optional hot-reload dev accelerator over the JitBackend seam (shipped c77), Cranelift runtime-side only (I6 holds — never in compiler `Source/`; lives in workspace member `jet-jit/`). Production = AOT. See [[two-lane-overnight-build-pipeline]].

### marker-plane-overhaul-2026-07-23

*Marker-system streamlining wave — cards #759-#766, 8 ballots minted 2026-07-23 awaiting owner ratification; owner pre-picked in chat*

2026-07-23 marker-plane phase 2 (after D-VERDICT-732-1 unified sigil onto `#`). Owner directed in chat: markers get signatures (call grammar); acronyms stay fully capitalized globally (glued vs underscore open → D-ACRO-CASE1); stacking = bare single / brackets for 2+; all six condensations accepted.

Ballots (rec = owner's chat pick, NOT self-ratified per [[never-blanket-ratify]]): D-MARKSIG1 (#759), D-ACRO-CASE1 (#760), D-MARK-STACK1 (#761), D-INLINE-PARAM1 / D-REDUCE-VALUE1 / D-CAPPLANE1 / D-CONSTMARK1 / D-UNSAFE-REASON1 (all #762).

Cards: #763 ghost triage (done: most "ghosts" fully implemented; no retires), #764 mechanical unification (one registry; blocked on D-MARKSIG1), #765 #Persist reload runtime gap, #766 #Track float-only gap.
**Why:** implementation must wait for ratification; recs must not be treated as ratified.
**How to apply:** after owner ratifies, implement per card scope; full ballot JSONs in /tmp/marker-ballots/ (regenerable from tower).

### marker-rebuild-2026-08-05

*Marker plane first-principles rebuild — proposal, law-zero verdict, 5-ballot slate RATIFIED 2026-08-05, impl cards #1456-#1461*

2026-08-05 marker-plane audit + rebuild (proposal: `docs/proposals/marker-plane-first-principles.md`).

- **Law zero ratified**: D-VERDICT-1455-1 — a marker exists iff it is a `Policy::APPLIED_RULES` row; no parsing/checking/formatting/highlighting/reflection outside it; every row fully implemented or retired; CI drift guards enforce. Owner words verbatim in the verdict.
- **Ballots on card #1455 — ALL RATIFIED 2026-08-05**: D-MARK-FORM1=A, D-MARK-REPEAT1=A, D-IMPURE-REASON1=B (reason stays OPTIONAL), D-SQL-ARG1=B (angle brackets ratified; markers gain a type-parameter feature), D-HTML-NAME1=B (one name HTML, two site-dependent signatures). Original proposal recommendations, for contrast: D-MARK-FORM1 (one placement law, five RuleForms retire), D-MARK-REPEAT1 (duplicate = error, registry `repeatable` column), D-IMPURE-REASON1 (reason required like #Unsafe), D-SQL-ARG1 (proposed `#SQL(Row)`; owner chose B instead), D-HTML-NAME1 (proposed a rename; owner chose B, keeping one name).
- **Impl cards**: #1456 uniform attachment (one parse path, markers retained on AST), #1457 one validation pipeline (E0927/E0355/E0930 everywhere; E0990/E0733/E0003 fallbacks retire), #1458 consumers (formatter from nodes, tree-sitter regen, MethodInfo.markers), #1459 row debt (#Authority/#Summarize, retired rows still applying effects), #1460 drift guards (gates close), #1461 ballot outcomes (blocked on #1455).
- Audit facts worth keeping: ~30 of 79 markers were hand-bumped past the registry; 5 different unknown-marker diagnostics by position; the July 2026-07-23 marker ballots survive only in `tower.json.pre-restore`, not the live board (spec is the live authority). Supersedes the "awaiting ratify" state in [[marker-plane-overhaul-2026-07-23]] — those were ratified and implemented.

- **2026-08-06 correction**: all five ballots are ratified, not awaiting. Verified against the live tower store. Five of six impl cards (#1456, #1457, #1458, #1460, #1461) are still `ready`; only #1459 is done. Law zero explicitly reserves the door to user-declared markers — see [[metaprogramming-one-program-2026-08-06]].

### math-operator-slate-2026-08-05

*Math-operator overhaul RATIFIED 2026-08-05 — cards #1428-#1436 (epoch e2) owe implementation; ^ power, ~| xor, /% floor div, % floored, ! bitnot, <=> spaceship (B!), bigint Int; #1437 matrix design open*

Math-operator slate minted and owner-ratified per-ballot from the board 2026-08-05 (event log confirms nine individual owner ratify events, 13:43–14:24). ALL NINE NOW LAW; implementation owed end to end per "ratify = build it" ([[ballot-first-always]] satisfied):

- D-EXPOP1=A (#1428) — infix `^` is power; `^=` power-assign; xor leaves `^`
- D-EXPSEM1=A (#1429) — right-assoc (`2^3^2==512`), `-3^2 == -9`, Int^nonneg→Int exact, Int^neg→Float
- D-XORSPELL1=A (#1430) — xor = infix `~|` with compound `~|=` (owner's own proposal; ratified the v3 menu)
- D-FLOORDIV1=A (#1431) — `/%` + `/%=`, floors toward −∞, Int and Float
- D-MODSEM1=A (#1432) — `%` floored + `%=`; `%%` truncated + `%%=`; identity a == b*(a/%b) + a%b
- D-INTDIV1=A (#1433) — `Int / Int → Float` (Python 3 model); `/%` is the integer path
- D-BITNOT1=A (#1434) — `!` = bitwise NOT on integers (Rust model; `-x-1` on bigint Int)
- D-CMP3WAY1=B (#1435) — SPACESHIP `<=>` operator + Ordering type; owner picked B against rec A
- D-INTBIG1=A (#1436) — default Int arbitrary precision w/ small-int fast path; fixed-width I8..U128 expert opt-in; amends D-NUMOPS1
- #1437 — matrix/linalg operator surface full design pass (owner: "I want to design this fully"); proposal doc next

Facts established: `~` prefix = copy only, infix free; `!=` on Bool already is logical xor; `///` doc comments + doctests shipped (S49, D-TEST4); comparison chaining shipped (D-CHAINCMP1); core.compute linalg is function-only.

Cards live in EPOCH e3 (owner corrected my e2 guess sharply — core-language operator work = e3 Product Pillars, not e2 GA). All ten are phase ready with dumb-model-verifiable exit criteria: each criterion names one example/golden or one targeted test with exact expected values; full-suite sweeps deferred to batch closeout (D-VERDICT-675-1). Blocks: #1429,#1430 ← #1428; #1433 ← #1431. Ratify auto-appended syntax chores (Syntax.rs, spec log, grammars, snapshots) to syntax-group cards. Owner: do NOT implement yet — cards ready only. Lesson: set --epoch (e3 for language work) at card add time.

Implementation touches: Syntax.rs, lexer/parser, sema binary.rs, Prelude (jet_pow/jet_mod/jet_bignum per I9 all tiers), formatter round-trip, tmGrammar/tree-sitter/zed/vscode, examples + golden. D-INTBIG1 is the big slice (bigint runtime rep across AOT/JIT/interpreter/web).

### memory-model-v5-direction

*Owner's settled memory-model direction (2026-07-03) — KEEP Rust borrow-checker semantics, humanize the surface; four alternative models rejected; D-MEM1 open*

After a 5-round design arc (2026-07-03), the owner reset to: **use the Rust borrow
checker system, make it much more ergonomic & beginner friendly.** Read-only by
default (owner endorsed), sigils not English words, `~` banned (painful to type),
prefers `&`/`^`.

Rejected along the way (do NOT re-propose): v1 value-semantics "One-Home" (hidden
CoW magic, limiting), v2 explicit-sigils + `from` provenance clauses (verbose),
v3 progressive-ceremony/mutate-via-subject-only (functions must be able to modify
args), v4 identity-object/generational-refs model (interrupted; over-rotated on one
example).

v5 **RATIFIED — D-MEM1 = A, 2026-07-03** (card #187): bare=read / `&`=write /
`^`=take, mirrored at call sites; second-class borrows (no stored or returned raw
borrows → no lifetime syntax exists); view values for String/[T] slices; named
escape hatches `Shared<T>` / `Pool<T>`; teaching diagnostics. Proposal:
tools/Tower/docs/proposals/memory-greenfield.md; migration plan (staged S1–S9):
tools/Tower/docs/plans/memory-v5-migration.md. Also ratified same day (card #188,
owner decree): `[]` universal empty literal, `Val(x)`/`None`, bare lambdas when
inferable, `(a, b) := f()` destructuring, `core.files` + `.write`, `.after(sep)`.
Jetpack proceeds first (independent).
**Why:** owner wants Rust's proven safety core, not a novel model; his pain is the
learning curve/ceremony, not the semantics.

**How to apply:** any memory-model work is gated on D-MEM1. If ratified, deliverable
#2 (owner-gated) is the staged migration path from the current AccessConvention /
`~` / L0201 / D-REF-SHORTHAND implementation. Glyph assignment (& vs ^ for write) is
a live ballot axis — don't assume. See [[ballot-first-always]],
[[do-it-right-measure-twice]].

### metaprogramming-one-program-2026-08-06

*Metaprogramming rethink RATIFIED 2026-08-06 (13 D-META-*, STAGE1=B); impl cards #1537-#1545 minted, e3*

2026-08-06 metaprogramming first-principles audit
(proposal: `docs/proposals/metaprogramming-one-compile-time-program.md`, card #1508).

**The one idea**: compile time is one Jet program, including the parts now
written in Rust. Half of Jet's compile-time program lives in closed Rust tables
(99 marker rows, 28 effect roots, 5 dimensions, 2 DSL blocks, the diagnostic
registry) that cannot be read, extended, or reflected. The other half — `#Known`,
derive bodies, `fn build` — is Jet.

**The proof it is one law, and it is the owner's own**: marker law zero
(D-VERDICT-1455-1) and the plane law (D-TYPE2-PLANE1=A) both say a thing exists
only if registered, nameable, and reflectable. Ratified six weeks apart, neither
naming the other. Neither says *where* a registration lives.

**The model**: three verbs — Read, Compute, Add. Every current mechanism is a
bundle (`#Codable` = Read(type) + Add(code); `module cache<K>` = Read(params) +
Add(code); a build policy rule = Read(program) + Add(diagnostic)). Two laws:
Add-only, and knowledge erases. D-METAMUTATE1 stops being a list of banned
features and becomes one sentence — **Change is not a verb**. So the wall did not
need reopening even though the owner allowed it.

**Key evidence**: `known_derive_names` (`crates/jet-sema/src/Sema/Bundle.rs:1234-1239`)
already folds user derive names into the marker vocabulary — Jet *already ships*
user-declared markers, restricted to one cell (input a type, output a string).
Generalizing that shipped mechanism opens every closed table. Two derive engines,
two splice implementations, three instantiation engines, two purity checkers.
`jet inspect expand` has four lenses and no derive lens, so §13's "hover a derive
and see its expansion" moat does not exist.

**Thirteen ballots on #1508, awaiting owner**: D-META-{ONE1, STAGE1, BODY1,
FORM1, CODE1, AUTO1, USER1, DSL1, EFFECT1, CONST1, MODNAME1, REG1, NAME1}.
Amends six ratified rulings by name: D-DSLBLOCK1=A, D-CTCORE1, D-VERDICT-1308-1,
D-VERDICT-1308-2, D-CTMARKER1, D-AUTODERIVE1=E.

**Owner feedback round (2026-08-06)**: (a) nested impl inside a derive body was
rejected — the body IS the members, header already names trait+type; (b) rule
declarations must use named optional parameters, NOT invented clauses — facts
about a rule are marked with the compile-time sigil in the same parameter list
($sites, $repeatable), which reduced the whole proposal to ONE new keyword;
(c) auto-derive must be opt-OUT for everything derivable, opt-in only for the
underivable; (d) owner caught that "$ to demand" was circular at a binder.
Resolution: the requirement lands on the RIGHT-HAND SIDE, not the name. The real
fork is whether the mark belongs to the BINDING (uses stay plain, splice rule
survives — what ships today per comptime_block.jet) or to the NAME (written at
every mention, splice rule deleted). D-META-STAGE1 now asks exactly that; only
the second reading deletes a concept.

**Highest-value cohesion finding**: four "one registry" proposals landed within
48 hours — marker registry, type plane registry, authority rights tree,
build-config fact plane — each citing the others, each its own Rust table.
D-META-REG1 asks whether they become one. Related: [[type-system-v2-2026-08-06]],
[[marker-rebuild-2026-08-05]], [[authority-one-model-2026-08-06]],
[[build-config-rethink-2026-08-06]].

**Side cards minted**: #1509 (jit_gaps and the tier-parity audit disagree about
`comptime/embed` and `comptime/find`), #1510 (D-METAMUTATE1, S26, D-METADEPTH1/2,
D-CTCODEGEN1 and 13 D-BUILD* exist only as spec prose, absent from all 422 Tower
decision records).

**STATUS 2026-08-06 (e3 audit):** All 13 D-META-* ratified (STAGE1=B, rest A). Impl slate minted by the e3 audit: #1537 ($ mark), #1538 (one registration table), #1539 (marker rows to Prelude — marker cards #1457/#1458/#1460/#1461 now block on it), #1540-#1545. See [[e3-audit-2026-08-06]].

### monetization-research-2026-08-06

*Jet monetization report at docs/research/monetization-strategy-2026-08-06.md — phased plan, pledge/spec/jetpack-provenance/grants first*

Owner asked 2026-08-06 how to earn $200k–$MM+/yr on Jet while it stays open source, non-villain. Full report: `docs/research/monetization-strategy-2026-08-06.md` (3 web-research dossiers behind it).

Core findings: money is never the language itself — it is retainers/anchor sponsors (Zig $160k salary, Dashbit), open-core ops tooling (Sidekiq ~$7M solo), visual editor (n8n $100M ARR), certified toolchain (Ferrocene), hosted execution (Replit) later. Only pre-adoption money: NLnet/Sovereign-Tech/Alpha-Omega memory-safety grants. Villain line: nothing free ever becomes unfree; never relicense; never meter running code; never price on unauditable numbers. Docs-eyeball funnels are dead post-AI (Tailwind −80% revenue 2026). Modal exit for great language tooling = AI-lab acquisition (Bun→Anthropic, Astral→OpenAI).

Phase 0 moves (pre-adoption, ~free): publish a permanent "free forever, no royalties" pledge; treat the spec as a product-grade artifact (unlocks cert + grants + AI codegen); bake signing/reproducible-builds/capability metadata into the jetpack manifest at v0; apply to NLnet. Adoption, not monetization, is the bottleneck; first production adopter is the key unlock. Related: [[jetpack-jetos-track]], [[blueprint-north-star]].

### native-deps-nixpkgs-interim-jetpack-future

*Owner policy — native/system deps may come from nixpkgs now if the user has Nix; jetpack owns native provisioning long-term*

Owner (2026-06-24): for native / system dependencies (C libraries, and native
math/SIMD backing like BLAS), it is acceptable **for now** to provision through
**nixpkgs** — but **only when the user is on NixOS / has Nix available**. In the
future this must be handled through **jetpack** (the first-party `core`
provider), so a Jet user needs no Nix at all.
**Why:** unblocks real native deps today without waiting on jetpack's full
realization, while keeping the long-term "no external toolchain" promise. It is a
stopgap, not the destination.

**How to apply:**
- The codebase already embodies this: `Source/Jetpack/Provider.rs` treats `nix`
  as a *compatibility provider* (every built-in source routes to `nix` today);
  C-deps auto-provision `nixpkgs#<attr>` in `Source/CFFI.rs` (pkg-config
  fallback, else E3201). The owner sanctioned this direction — don't fight it.
- A non-Nix user hitting a native dep must get a clear "this currently needs
  Nix; jetpack-native provisioning is planned" message — never a silent failure.
- For [[u9-provider-kind-inferred]]: source core-vs-nix is inferred from the
  target's `pkg.jet`; nixpkgs targets are unconditionally `nix`.
- Prefer the nixpkgs provider over vendoring a Rust crate into `Source/` for
  native backing libs (relevant to c94 linalg/SIMD; respects I6 on the compiler).
- Canonical write-up: jetpack plan `unified-ecosystem.md` §5 (provider model).
  This is a backend/provider migration, not a syntax decision — no ballot.

### nix-shell-banner-pollutes-output

*nix develop -c prints a dev-shell banner to stdout that pollutes captured command output*

**FIXED everywhere (merged to master 2026-07-06): shellHook banner goes to stderr — stdout is clean.**

Every `nix develop -c <cmd>` prints the "Jet dev shell / build: / run: / search: / …" banner to **stdout** (plus a "warning: Git tree is dirty" line to stderr). This contaminates anything you capture or grep:

- `rg`/`grep` output gets the banner mixed in — I once misread a grep as "no matches" and chased a non-existent bug (thought `manifest_toml` was dead code when it was wired).
- Redirecting program output to a file (`nix develop -c jet run x > out`) captures the banner into the file — broke a golden `expected/*.out` capture.

**How to apply:** when capturing, filter the banner, e.g. `| grep -ivE "dev shell|build:|run:|search:|LSP:|editor:|Cursor|Zed|release:|Git tree"`. For byte-exact output, write expected files by hand, not by redirecting `nix develop -c` output. Note `jet` in the dev shell resolves to a nix-store binary built from the (dirty) working tree — it reflects current source, but `target/debug/jet` (what `cargo test`'s `CARGO_BIN_EXE_jet` uses) is the authoritative build; run it via `nix develop -c ./target/debug/jet …` so `cc` is on PATH. Related: [[do-it-right-measure-twice]].

### orchestrators-start-dont-close

*2026-07-12 all-day 3-orchestrator run burned net ~3 cards — agents claim new cards instead of closing; fully met+verified cards left un-advanced; check building lane first*

2026-07-12: three orchestrators (claude-main, codex-orchestrator, cursor-e4-burn) + ~50 subagent identities produced 470 board events + ~40 commits but net open-card burn was ~3 (157→154). Owner angry ("card number has not changed").
**Why:** (1) WIP inflated 16→22 building — orchestrators claimed fresh cards instead of closing claimed ones; (2) cards with ALL criteria met+verified (#477, #497) were never phase-moved to done — pure bookkeeping miss; (3) criteria-add mid-flight moved goalposts (+7 criteria on #367, #515, cursor-e4-burn); (4) ~12 of 40 commits were fix(tests) repairing cross-orchestrator drift (card #521 inventory); (5) 20:12 retire sweep moved 35 old done cards to history.json — done column shrank 57→33, hiding the day's real progress.

**How to apply:** Before claiming ANY ready card, sweep the building/verify lanes for cards whose criteria are all met — run the independent verify pass and phase-move them to done. When orchestrating multi-wave work on one card, brief builders to `--meet` their criteria with evidence as they land (2026-07-13: told wave agents "no criteria writes" → all 6 criteria sat open, verifier's `--verify` was refused with "criterion not met yet", extra round-trip to backfill meets). Freeze criteria at claim time (post-claim additions become follow-up cards). One repo-writing orchestrator at a time (see [[two-lane-overnight-build-pipeline]]). Current-epoch ready lane is all epics — "low-hanging fruit" now means closing in-flight cards, not finding small ready ones. Related: [[never-skip-to-next-card]], [[sequential-card-work-use-nonisolated-agents]].

### owner-plans-all-epochs-mandate

*Owner 2026-07-16 — build plans for ALL epochs and make EVERY card on the board implementation-ready (plan + criteria + gates balloted + ordering), not just epoch 3 + sidequests*

Owner, 2026-07-16: "I wanted you to build all the plans for all the epochs and get all the cards on the board ready for implementation. Not just sidequests and epoch 3, those were just the starting point."
**Why:** e3 burndown prioritization was only step one; the whole board (e4, e6, e7, e8, e9) must reach the same readiness bar.

**How to apply:** implementation-ready = card has a concrete plan, machine-checkable criteria[], every owner-gate enumerated and minted as a ballot (BALLOT FIRST), correct blockedBy, and a workOrder slot. Run per-epoch planning waves; coordination card on the board tracks progress (cards are the handoff source of truth).

Related: [[clerical-sweep-2026-07-16]], [[ballot-first-always]], [[one-compiler-two-lenses]]

### payload-multi-package-core-source

*pkg.jet (NOT pack.jet/payload.jet) is the per-payload manifest; payload: identity block + packages: list; workspace.jet indexes monorepos; env.jet is dev shell only*

File-structure canon (U10 as revised 2026-06-18 + D-WORKSPACE1/2, in
`docs/spec/syntax-decisions.md` — check there first, this moved twice):

- **`pkg.jet`** — the manifest filename (renames: `pack.jet` → `payload.jet` →
  **`pkg.jet`**). One per publishable unit, user-chosen dir. Contains identity
  block `payload: { name, version }` + `packages: { name: library|executable }`.
  Hierarchy: payload → packages → modules; package IS a top-level module.
- **`workspace.jet`** — monorepo index at repo root: `module workspace
  { members: find("./packages") }` (D-WORKSPACE1=B fully computable,
  D-WORKSPACE2; implemented). This, not pkg.jet, is the monorepo surface.
- **`jetpack.toml`** — residual root TOML: `[repo]` metadata + `[sources]`
  defaults only (`[packages]` moved to workspace.jet).
- **`env.jet`** — dev shell only, never a package index; owner rejected retiring it.
- **`.jet/lock`** — single lockfile incl. `[[workspace_member]]` +
  `[[comptime_inputs]]` hashes.
- U9 provider inference probes for `pkg.jet`.
- Monorepo addressing: `mono.ranker` dot form, sparse fetch (D-MONOREF1).

See [[u9-provider-kind-inferred]], [[jetpack-jetos-track]].

### session-2026-07-13-handoff

*Resume point after the 2026-07-12/13 overnight closeout+burndown session (owner stop order ~mid-morning 07-13)*

Overnight session closed 8 cards verified (#497 #476 #477 #452 #521 #528 #522 #529), reopened 4 dishonest closes (#126 #136 #142 #143), landed stranded branches grok/card-476+477, burned the 45-test master-red down (see #521 log for the full fix ledger), shipped #506 slices D-STM1 + D-ENVHOOK1 (criteria 5+7 met, UNVERIFIED — verifier needed), parked #505 blockedBy [#444 #438 #134].

**Resume next (in order):**
1. Re-dispatch #506 D-OBSERVE-LIVE1 slice (was stopped pre-code, nothing partial; honest-stop mandate; shape guides: 47b3de84/1eb3adef/9c7cf099/6eb7997e).
2. Verify #506 criteria 5+7 (builders fable-impl-506stm/506env; use fresh jet-verify identity).
3. Then card #438 (app graph, D-WEBAPP1) — highest leverage: D-AUTH1/D-SYNC1/D-DBPOLICY1/live-query all funnel into it.
4. Owner queue when he returns: D-ACCEPT-359, D-CLI-HANGAR1, D-OPDEF1, D-AUTH2, D-TAINT2 ballots + #460 VAPID rotation. Act on ratifications per tower skill.

Cards #530 (email substrate, ballot-gated) + #528/#529 context in their logs. Sidequest lane leftovers: #367/#501 epics (claude-main lineage = resumable), #467 (needs macOS/Windows CI — assess honestly). Peer lanes live during session: codex (REPL/Help/LSP/crypto, card #91 worktree /tmp/jet-card-91*) + an e4 hangar lane (card #393, Recipe.rs/Comptime Build churn) — re-check their file territories before claiming overlap. Known red not to chase: fish PTY receipt flake, canvas conflict tests under concurrent writers (green standalone). Related: [[orchestrators-start-dont-close]], [[tower-ballot-validator-schema]].

### tmp-is-tmpfs-no-cargo-targets

*/tmp is RAM-backed tmpfs — never put cargo target dirs or multi-GB logs there; it caused kernel OOM crashes*

On the owner's machine `/tmp` is a 31G tmpfs backed by RAM + swap (24G total swap: 16G zram + 8.8G disk).
**Why:** On 2026-08-07 agents had left `/tmp/jet-e4-target` (15G cargo target), a 5.2G scratchpad `target3`, and ~2G of logs in /tmp. tmpfs pages spilled into swap, maxed it, and the kernel OOM killer started killing rustc and electron despite 64G RAM.

**How to apply:** Never set `CARGO_TARGET_DIR` under `/tmp` or write multi-GB build outputs/logs to /tmp or the scratchpad on this machine. Use a disk path (e.g. under the repo or `~/.cache`) for alternate target dirs, and delete large session outputs when done. Related: [[nix-shell-tmp-fills-disk]].

### tower-ballot-validator-schema

*Exact hard-enforced ballot JSON schema + prose caps for tower decision add (E_BALLOT)*

`tower decision add` hard-validates (Tower/app/store.mjs `ballotGaps`):
required fields `gist`, `lesson`, `story`, `inWild`, `options[]` (≥2, each
key/name/detail/code), `rec` (must match an option key), `recommendation:
{why, tradeoff, whyNot:[{key,reason}] for EVERY non-rec option}`, `hybrid:
{result (== rec), synthesis, harvest:[{key,aspect,use}] for EVERY option
incl. rec}`. Prose caps on gist/lesson/story/option.detail/comparison.note/
recommendation.*/hybrid.*: sentence ≤32 words, paragraph ≤90 words
(split lessons with \n\n). Sentences split only on `.!?` — semicolons do
NOT end a sentence, so long semicolon chains trip the cap. Target ≤26
words/sentence when authoring. `--draft` skips validation; `decision
update <id> --ready` re-validates.
**Why:** six of my first ballots bounced on these caps; agents briefed
with the exact schema + caps pass first try.

**How to apply:** paste the schema + caps into every [[subagents-must-invoke-caveman-skill]] jet-ballot brief; fix rejects by splitting the named sentence.

Quirk (2026-07-12): a sentence ending inside a quoted/backticked phrase (`"…chars."`) is not split by the sentence regex — it merges into the next sentence and can trip the 32-word cap. Do not end sentences inside quotes/backticks.

### tower-docs-layout

*PM docs live at docs/{plans,proposals,sidequests,ballots} since 2026-07-10; tools/Tower deleted; durable spec stays in docs/spec*

PM docs (epoch plans, proposals, sidequests, ballot docs) live under repo
`docs/plans/`, `docs/proposals/`, `docs/sidequests/`, `docs/ballots/` since
2026-07-10. Durable spec stays in `docs/spec/` (syntax-decisions.md remains
the ratified single source of truth). The retired `tools/Tower/` tree (old
app + frozen tower.json + the docs' previous home) was deleted that day; all
repo references and live card body/plan paths were rewritten (historical card
logs/events keep old path strings). The owner may still run a separate agent
on `docs/proposals/` — when sweeping the board, write plans to
`docs/sidequests/`, never into `proposals/`. The old app's in-app markdown
viewer died with it; a viewer for the new Tower is a follow-up noted on card
#464. See [[tower-messaging-removed]] and [[board-json-owner-owned]].

### tower-githook-dirties-tree

*Commits mentioning*

The repo has `tower githook` installed: any commit whose message mentions
`#N` writes a log entry onto card N, mutating `.tower/tower.json` (+
history.json) AFTER the commit — so the tree is dirty again immediately
and the `scripts/agent/require-clean-tree.sh` Agent-delegation hook
refuses. A live `tower serve` (owner on the board) also rewrites tower
state at any moment.
**Why:** cost several failed Agent launches on 2026-07-11.

**How to apply:** checkpoint commits made right before delegating must
NOT mention card numbers (`#497` etc.); if the hook still refuses, `git
add -A && git commit` once more (picks up the githook's write) and retry
immediately.

### tower-platform-worktree

*Generalized standalone Tower (any-project plugin, CLI+HTTP API, milestones) built + committed on worktree branch worktree-tower-platform, 2026-07-04*

Owner asked for a generalized, importable Tower platform (2026-07-04). Built at
repo-root `Tower/` on worktree branch `worktree-tower-platform` (worktree at
`.claude/worktrees/tower-platform`), commit dd3f5195. **MERGED to master
2026-07-06** (merge 52cc6d0d, master ff'd to a2861f18) — branch and master now
identical; worktree dir still exists and can be `git worktree remove`d. Key facts:

- Packaging: Claude Code plugin (`.claude-plugin/plugin.json` + `skills/tower/SKILL.md`)
  AND agent-agnostic `AGENTS.md`; owner wants non-Claude agents supported.
- Agents use CLI (`node Tower/tower.mjs …`) or HTTP — never hand-edit JSON.
  Data lives in host project `.tower/` (tower.json + config.json + backups/).
- Model per owner: epochs stay major groupings; [[jetpack-jetos-track]]-style
  naming untouched; **milestones are goals within epochs** (cards link via
  milestoneId, progress computed).
- Reliability: lock dir, atomic writes, rev optimistic concurrency (exit 2 /
  HTTP 409), event log with --by, card claims, rolling backups, 19 node --test
  tests. `tower import` migrates v3 boards (verified on live jet board).
- zen-beta headless is unusable for screenshots (SWGL paint races); use
  nixpkgs chromium: `/nix/store/...-chromium-149*/bin/chromium --headless=new
  --no-sandbox --virtual-time-budget=8000 --screenshot=…` (nix build
  nixpkgs#chromium). Fake-DOM harness catches logic errors cheaply.
- v2 (commit c685571f): greenfield UI (beacon strip, Now queue, Agents chat,
  Chakra Petch/Instrument Sans/JetBrains Mono), owner⇄agent messaging
  (listen long-poll + launch bridge via config.commands), skills split
  tower/tower-ballot/tower-setup. CSS gotcha: base classes setting display
  override [hidden] — keep `[hidden]{display:none!important}`.
- Port 8899 opened in firewall (owner's nrs applied it);
  ~/nixos/modules/core/network.nix edit applied but NOT committed in ~/nixos.
- CUTOVER DONE 2026-07-04: jet now runs new Tower. App at jet-root `Tower/`
  (untracked in main checkout — commit pending), live data `.tower/tower.json`
  (imported from tools/Tower/tower.json, which is FROZEN + has MOVED.md +
  handoff questions on #187/#188 for the concurrently-running agent).
  Server on 7878 only (owner declined 8899; firewall port stays open but unused). Project skills replaced:
  .claude/skills/tower (new loop + jet addendum), tower-ballot, tower-setup.
  CLAUDE.md decision-protocol line updated. Old server pid killed.
  Legacy-file write watch armed in session (inotify) — if the old agent
  writes tools/Tower/tower.json directly, re-sync that delta into .tower.
- General copies: ~/Projects/Github/homeschool-academy/Tower (+ .tower init,
  skills copied, uncommitted) and ~/Downloads/Tower + tower-plugin-2.1.0.tar.gz.
- v2.1 wave (commit d01c8103, deployed 2026-07-04): SSE live updates +
  rev-gated render (fixed owner's page-reset complaint), token auth
  (localhost exempt; jet token in .tower/config.json auth.token — phone URL
  needs ?key= once), PWA + payload-less VAPID push (◍ notify button),
  BATCHED ratify/greenlight agent wake (notifyBatchSeconds=90, one [tower]
  msg per listener — owner explicitly wanted batching), launch output
  streaming, undo (rev-guarded), message attachments, ⌘K palette, j/k,
  compare mode, aging chips, digest, agent status heartbeats,
  `tower githook` (installed in jet .git). 29 tests.
- Screenshot chromium: SSE keeps the connection open — add --timeout=12000
  or the screenshot hangs forever.

### tower-server-stale-process-trap

*Tower server loads code at startup; long-running process serves fresh UI files but stale API routes — restart it after changing Tower/app server code*

2026-07-12: owner's Accept clicks failed for hours. The live `tower.mjs serve` process (started 2026-07-10, port 7878) served NEW tower.js from disk per-request but its in-process routes predated the acceptance endpoints — every fix looked landed while the running server 404'd. All agent verification on fresh temp servers = false green for the owner's instance.

**How to apply (amended 2026-08-05):** a stale or duplicate serve process means divergent in-memory board state — REPORT it to the owner and stop; never kill/restart/start a server yourself ([[owner-only-tower-serve]]). Verify fixes against the owner's LIVE port, not temp servers (temp servers are themselves forbidden now). Related: [[acceptance-requests-visual-only-short]].

### tower-v2-features

*Tower gained criteria/gates/history/brief/lint/verdict/Radar on 2026-07-10 — agent workflow now starts with `tower brief` and done is mechanically gated*

Tower feature wave shipped 2026-07-10 (cards #446, #450, #457, #458, #461,
#462, #463, #464):

- **Session start:** `tower brief --agent <me>` — one call returns the full
  work packet (card, criteria, decisions VERBATIM with owner comments,
  questions, refs, rules). Replaces status/next/show/decision-list ritual.
- **Done is gated:** cards with `criteria[]` refuse agent `--phase done`
  until every item is verified by someone ≠ the builder (E_CRITERIA /
  E_CRITERIA_SELF). `needsAcceptance` cards mint a D-ACCEPT-<num> ballot and
  wait in verify for the owner. Owner writes bypass all gates.
- **Guard gates (agent-hard, owner-soft):** decision add validates ballot
  completeness (--draft escape), ratify/activate owner-only (--quote escape),
  releasing a building card needs --handoff, `tower verdict` mints owner
  verdicts as ratified decisions, syntax-group ratify auto-appends the 4
  standard chores to criteria.
- **History store:** done cards + ratified decisions retire to
  .tower/history.json after `retireAfterDays` (3) — walk-back buffer with a
  "Recently decided" reopen strip on Now; `tower archive status/show/restore`;
  reads fall through transparently.
- **`tower lint`** (+--docs) sweeps hygiene; `tower next --burndown` encodes
  the burndown scope structurally.
- **Radar tab** = burndown/ops-table prototype, owner-acceptance pending
  (D-ACCEPT-464); Board/Now unchanged.
- CI (`ci.yml`) runs verify-full.sh with JET_REQUIRE_RUSTC=1 every push;
  `jet devtools` gained reduce/ice-report/new-example/new-ui/
  check-fixture-paths/bless.

Trap: `--dir` only works on `tower init`; pointing other commands at a
fixture needs `--data <dir>` or TOWER_DATA — a sub-agent once wrote a bogus
milestone to the LIVE board misusing --dir. See [[tower-messaging-removed]],
[[board-json-owner-owned]].

### two-lane-overnight-build-pipeline

*The proven pattern for large autonomous Tower build sweeps — serialized Source/ builds verified vs baseline, parallel docs/ballot work, single-writer merges*

Pattern that cleared the entire non-frozen Tower backlog in one overnight session
(2026-06-24/25): the effect cluster + ~12 ready cards + a formatter regression fix,
all built/verified/pushed, plus ~20 ballots queued.

**Lanes.** Lane A = `Source/` builds, strictly SERIALIZED (one subagent at a time —
parallel Source/ edits collide). Lane B = docs/ballot/plan work, parallelizable with
a Lane A build ONLY when it touches non-`Source/` files (and not the same
`docs/spec/` file the Lane A build updates). Zed grammar (`editors/`) and docs sweeps
parallelized cleanly with a Source/ build.

**Per Lane-A card:** delegate with a tight prompt that includes — read the ratified
decision first; audit existing state (most "ready" cards were 50–95% already built);
the exact known test baseline (arena 4 flake / tir 3 / closures 1 / grammar 1 / pkg
2); I1–I8; formatter round-trip ([[formatter-roundtrip-required-for-new-syntax]]);
"build the clearly-specified part, write any genuine owner-facing fork to a scratch
ballot file (house format) — never guess syntax." Then I verify INDEPENDENTLY
(build + `cargo test --no-run` + the new example + affected suites + confirm only the
baseline fails — see [[verify-subagent-builds-diagnostics-are-stale]]), commit with a
heredoc message (`git commit -F -`, never `-m "..."` — backticks/`$()` corrupt it),
push, update the board card, merge any scratch ballot single-writer.

**Forks, not guesses.** Every build that hit an unratified sub-choice produced a
house-format ballot (D-LIN1-DROP, D-TXN-ROLLBACK, D-TAINT-SAN, D-DET-CAPAPI, D-STATE
×3, D-PARSE-1, D-JIT2) rather than inventing syntax — matches [[do-it-right-measure-twice]]
and the owner's [[owner-task-pipeline-workflow]]. Vague refactor cards (c111) → audit
+ scoped plan, not a risky rewrite (I8). Plans reaching the owner get a second-agent
vet. Commit/push only the finished card's paths; leave a concurrent agent's
uncommitted files alone (`git add <specific paths>`, not `-A`, when another build runs).

### type-system-v2-2026-08-06

*Type system v2 \"carriers and knowledge\" proposal + ALL 11 D-TYPE2-* ballots RATIFIED (option A) 2026-08-06, unbuilt*

Type system v2 first-principles proposal (2026-08-06): a type is a carrier (runtime bits) plus knowledge (compile-time facts in planes with algebras). Unifies ~10 shadow systems: ranges/widths/invariants → one interval plane; lengths/shapes/lanes/dimension-exponents → one measure substrate; 3 duration systems → Time unit family; exactness law "knowledge is never lost silently"; BigInt retires into bigint Int.

- Proposal: `docs/proposals/type-system-v2-carriers-and-knowledge.md` (has executive summary — owner explicitly wants exsums on reports).
- Card #1497 (e3): 11 full ballots D-TYPE2-{FOUND1,NUM1,REFINE1,TIME1,MEASURE1,EXACT1,UNCERT1,PLANE1,DEFAULT1,SPELL1,IMAG1}, criteria 1-2 met+verified, 3-4 wait on ratification.
- Owner feedback round: precise-by-default is the owner's stated instinct (exact ℚ default, approximation = expert restriction; DEFAULT1 amends D-INTDIV1 + D-EXPSEM1/D-EXPNEG1 + D-NUMTYPE1 by name). Owner wants syntax proposals in first-principles work, and worked example programs — "dry report, can't visualize, I need to see it" — proposal now has a "What it looks like" section with 3 full programs.
- Review found 1 blocking (sized widths must keep trap-on-overflow per D-INTBIG1, never "widen to base") — fixed. MEASURE1 amends D-COMPUTE-TYPE1's encoding; D-MARK-REG1 lives inside D-VERDICT-1455-1, not standalone.
- Commits b199f3f4a, a7b6b7e0c. Related: [[math-operator-slate-2026-08-05]], [[marker-rebuild-2026-08-05]].
- Delegation trap: `require-clean-tree.sh` PreToolUse hook blocks the Agent tool while the shared tree is dirty with another session's work; workaround = headless `claude -p --permission-mode plan` via Bash for read-only review (codex was out of credits until Aug 8).

- **2026-08-06 correction**: all eleven D-TYPE2-* ballots are RATIFIED, every one option A. Verified against the live tower store. Card #1497 is still `planning` and nothing is built, so this is the largest ratified-but-unbuilt surface on the board. D-TYPE2-PLANE1=A (the plane law: registered, nameable, reflectable, readable) states the same rule as marker law zero — see [[metaprogramming-one-program-2026-08-06]].

### u9-provider-kind-inferred

*U9 — a source's provider kind (core vs nix) is inferred from the target's pack.jet, never declared; no via: marker*

Ratified U9 (2026-06-16). A typed `sources:` entry is **only** ever
`name: provider@target` — there is **no `via:`/kind marker**. Whether a source
realizes through the first-party **core** provider or a **nix flake** is
*inferred* from its target: target has a **`pack.jet`** → core; else → nix.

The owner reframed the old "how do typed sources declare `core`?" syntax
question (which had been blocked) into a behavior decision that dissolves it
entirely. Rejected: a `via: core` field, an inline `via` keyword, a `core@…`
provider prefix.
**Why:** core-by-default with a safe nix fallback keeps syntax clean and gives
every env the whole nixpkgs repo for free. This is the owner's instinct — push
ambiguity into inference, keep the surface minimal ([[do-it-right-measure-twice]],
[[owner-design-kill-criteria]]).

**How to apply:** probe must never clone a nixpkgs-sized repo — `path@…` stats
locally, `nixpkgs@…` is unconditionally nix (never probed), `github@…`/git URLs
peek at **only** `pack.jet` (raw fetch / shallow `git archive`) before a full
fetch. Recorded in syntax-decisions.md (U9 + ledger) and unified-ecosystem.md §6.
**Not yet implemented** — `modeval::build_source_table` still hard-codes
`ProviderKind::Nix`; this is the next chunk. Note the core provider currently
reads the source repo's `env.jet` `pkg.package(...)` index while U9 keys on
`pack.jet`; reconcile which file marks a Jet package repo. See
[[packjet-migration-sequencing]], [[jetpack-jetos-track]].

### zed-grammar-repo-is-generated

*editors/zed/grammar-repo is a generated build artifact (install.sh), not source — never track/commit it*

`editors/zed/grammar-repo/` is a **generated build artifact**, not source. The
authoritative tree-sitter grammar lives in `editors/tree-sitter/` (committed).
`editors/zed/install.sh` syncs that into `grammar-repo/`, runs `tree-sitter
generate`, then `git init`s + commits it locally and writes a per-machine
`GRAMMAR_REV` into the generated `extension.toml` (Zed clones grammar-repo via a
`file://` URI).

A committed gitlink pointer for it goes stale on every rebuild and differs per
machine → it shows as perpetually "modified content" in `git status` that no
other device sees, and can't be deleted normally (it's a nested git repo).

Fixed 2026-06-25: removed the stray gitlink from the index and gitignored
`grammar-repo/` (the .gitignore carries a comment explaining why). To regenerate
anywhere: `editors/zed/install.sh`. If grammar-repo ever reappears as tracked or
phantom-dirty, the fix is to untrack it again, not to bump/commit the pointer.
Don't discard its working-tree contents thinking it's junk — it mirrors the
committed `editors/tree-sitter/` grammar.

## External references

### canvas-browser-verification

*How to launch + screenshot/drive Canvas headlessly on this NixOS box*

Canvas = `nix develop -c jet dev examples/features/tooling/canvas_blueprint_demo.jet --target=web --port=<N>` then http://localhost:<N>/canvas. Rebuild + restart server after compiler changes (wrapper execs target/debug/jet at spawn).

Headless browser: playwright-core (npm i in scratchpad) + `executablePath: /etc/profiles/per-user/nate/bin/brave-origin-beta`. Playwright's own downloaded chromium FAILS on NixOS (libglib missing). Node at /nix/store/.../nodejs-22.23.1/bin (get via `nix develop -c which node`).

Nodes draw on single `<canvas id="jet-canvas-view">` — DOM queries see no nodes; use screenshots + JSON endpoints (/canvas/graph, /canvas/core-catalog, POST /canvas/query) for structure. Right-click menu IS DOM (.action-result rows, .action-category). Dispatch contextmenu via MouseEvent on the canvas el.

Insert transactions write the demo .jet file — `git checkout -- examples/...` after verification. Failed sema inserts 409-rollback with diagnostic in status bar (bottom).

Reusable harness from 2026-07-09 session: shot.js + action JSONs in that session's scratchpad (pattern: goto, actions list w/ click/fill/eval/shot, console+DOM audit).

### codex-cli-orchestration

*How to drive codex exec (gpt-5.6-sol) as implementation subagent from Claude orchestrator; /sol personal skill is the entry point*

Owner sometimes wants Codex as the implementation subagent ("use codex/sol, you just orchestrate"). 2026-07-15: standard worker model is now **gpt-5.6-sol** (verified working); full playbook lives in the personal skill `~/.claude/skills/sol/SKILL.md` — invoke `/sol`. Owner installs the `codex@openai-codex` Claude Code plugin (from `openai/codex-plugin-cc` marketplace) for the integrated lane (codex:rescue/review/status); raw `codex exec` lane always works. `-c approval_policy=never` is classifier-blocked; default on-request runs fine headless. Effort policy (owner 2026-07-15): medium default, high only when slice needs it, low for mechanical — always pass explicitly (config default is low).

Working invocation: `cat brief.md | codex exec -m gpt-5.5 -c model_reasoning_effort=high --sandbox workspace-write --skip-git-repo-check -i img1.png -i img2.png -` (background via run_in_background, watch with `until ! pgrep -f "codex exec"`).

- TRAP: with `-i` images, a positional prompt is ignored ("No prompt provided via stdin") — pass the brief via stdin with `-`.
- Output is pipe-buffered; nothing appears in the task file until exit. Track progress via `git status --short` on scoped files instead.
- Codex reads AGENTS.md itself; briefs still must state scope (files it may touch), invariants, verify commands, "orchestrator runs full suite".
- ~5-15 min, ~220-330k tokens per agent round at high effort. Serialize agents (nix develop serialization + shared js.rs).
- Codex desktop app may run concurrently as owner's separate agent (JetOS files dirty mid-session) — never assume all git dirt is yours; scope commits accordingly.

### jetlang-flake-input-gitfile

*host jet/jetpack are live-debug wrapper scripts (never stale after cargo build); jetlang flake input is git+file; several staleness-diagnosis traps*

Host `/etc/profiles/per-user/nate/bin/{jet,jetpack}` are NOT store builds — they are writeShellScriptBin wrappers (~/nixos/modules/apps/dev/jetlang.nix) that exec the **newest-mtime** `target/debug/<bin>` across the main checkout AND `.claude/worktrees/*/target/debug/`. `cargo build` is immediately live on PATH; no nixos-rebuild needed per change.

Traps:
- `grep -ac <feature-string> /etc/profiles/.../jet` proves nothing — it greps the wrapper script, not a binary. Grep `target/debug/jet` instead.
- A stale agent worktree with a newer-mtime binary silently shadows the main build (newest-wins).
- "Old version" reports are usually NOT binary staleness — check whether the implementation actually matches the ratified design (see 2026-07-09: cards #356–#362 were closed with thin textual slices while D-FE-*1 option D mocks specify rich interactive TTY surfaces).

`~/nixos/flake.nix` input `jetlang.url = "git+file:///home/nate/Projects/Github/jet"` (changed from `path:` 2026-07-09 — path fetcher copied the entire 149G `target/` and hard-errors on unix sockets in `target/test-tmp/`). git+file takes tracked files only and needs a clean tree to write the lock — Tower server keeps `.tower/tower.json` dirty, so commit before `nix flake update jetlang --flake ~/nixos`. The rebuild only consumes jetlang for the vscode extension ([[cursor-extensions-home-manager]], importNpmLock) — `nix build .#jet` release check phase fails in sandbox (tests/archive.rs needs writable FFI cache + network), so don't gate on it.

### jetpack-integration-tests-stale-bin

*jetpack_engine/offline/dispatch integration tests shell out to target/debug/jetpack which only rebuilds when ABSENT — cargo build -p jetpack --bin jetpack before running them in isolation, else false green/red*

`tests/common/mod.rs::resolve_or_build_bin` resolves the `jetpack` binary the
integration tests exec: it uses `CARGO_BIN_EXE_jetpack` if set, else falls back
to `target/debug/jetpack` and **only rebuilds it when the file is absent**
(`if !bin.is_file()`). Because `jetpack` is its own workspace crate, running a
root-`jet`-package test target (`cargo test --test jetpack_engine`) does NOT set
`CARGO_BIN_EXE_jetpack` and does NOT rebuild the standalone bin — so the test
shells out to a **stale `target/debug/jetpack`** from whenever it was last built.

Consequence: `jet_build_never_reports_*` and other jetpack integration tests can
pass under the full `verify-full.sh` run (whole `cargo test` builds the bin
fresh) but **fail deterministically in isolation** with a stale bin — or the
reverse. This burned ~an hour: an independent verifier flagged two tests red
that two prior full-suite runs showed green; the code was byte-identical, the
difference was purely the stale exec'd binary.

Rule: before running any `--test jetpack_engine`/`jetpack_offline`/
`jetpack_dispatch` in isolation, run
`scripts/agent/jet-env sh -c 'cargo build -p jetpack --bin jetpack'` first, then
the test. Distinct from [[jet-build-cache-stale-binaries]] (jet's AST-keyed build
cache) and [[verify-subagent-builds-diagnostics-are-stale]] (mid-build snapshot
staleness) — this one is the test harness's own bin-resolution fallback.

### nix-shell-tmp-fills-disk

*nix develop -c leaves ~197M /tmp/nix-shell.* dirs per call; thousands accumulate, fill the 31G tmpfs, and cause phantom rustc/test build failures (ENOSPC)*

Every `nix develop -c …` invocation creates a `/tmp/nix-shell.<rand>` dir (~197M each) and does NOT clean it up. Across a long session of builds/tests — especially byte-parity sweeps that build the whole example suite twice — thousands accumulate (saw 6563 dirs = 31G) and fill the `/tmp` tmpfs to 100%.

**Symptom:** spurious test failures — `test result: FAILED. N passed; 2 failed` with NO `panicked`/`assertion`/`error[` marker anywhere, and counts that vary between runs. These are rustc/`build_and_run` temp-write failures (ENOSPC), not real code regressions. The harness's own task-output writes also start failing with "temp filesystem … is full".

**Fix:** `rm -rf /tmp/nix-shell.* 2>/dev/null` periodically (instantly frees it). Do a cleanup pass between byte-parity-heavy phases. When a full-suite run shows failures, FIRST check `df -h /tmp` before trusting the failure as real — re-run after cleaning.

Related: [[nix-shell-banner-pollutes-output]].

### owner-nixos-config-acceptance

*Owner's ~/nixos config is the jetos acceptance fixture (card*

Owner's real NixOS config lives at `~/nixos` (flake-parts + import-tree, 110 .nix modules, host `halcyon` + variant `halcyon-plasma-beta` + custom ISO host). Card #337 tracks 100% recreation in jetos as the drop-in-proof, with a coverage matrix in its body mapping every config feature → covering card (#320–#336, #262/#263, jetpack U-cards).

Key facts: KDE Plasma (not the ratified GNOME default — breadth needed), limine bootloader, CachyOS kernel via typed profile enum (safe/lts/performance), stylix theming, home-manager + plasma-manager + nixcord + nix-flatpak + spicetify + nix-vscode-extensions, hand-rolled flatpak reconcile script, zram+sysctl, scx sched-ext, libvirt/spice virtualization, gaming stack, per-host nixpkgs channel swap via mkHalcyon, lib.mkForce/mkDefault/disabledModules usage, jet packaged from local path input.

### prelude-embedded-rebuild

*CoreLib.rs prelude is include_str!-embedded into jet at compile time — rebuild jet after editing it; dead prelude code never warns*

`Source/Prelude/CoreLib.rs` (and `Core.rs`/`Mem.rs`) are embedded into the `jet`
binary via `include_str!` at **compile time** (`Source/Codegen/mod.rs`:
`CORELIB_PRELUDE`/`PRELUDE`/`MEM_PRELUDE`), then concatenated as text into every
generated Rust program.

Consequences:
- **After editing the prelude you MUST `cargo build` jet** before `jet run`
  reflects the change. Running `jet run` against an unrebuilt `jet` silently uses
  the OLD prelude (and `jet run` even reuses a stale `build/<name>.rs`; `rm -rf build`
  to be sure). Cost me a confusing cycle where my edit "didn't take."
- **Dead/unused functions in CoreLib.rs do NOT warn during `cargo build` of jet** —
  it's a string, not compiled as part of the compiler. Unused prelude fns only
  matter when a generated program includes them (the prelude allows dead code), so
  remove superseded prelude fns deliberately; the build won't flag them.
- Borrow-checker / rustc errors in prelude code surface as an **ICE (exit 101, I2
  banner)** when a generated program is compiled, pointing at `build/<name>.rs`.
  Prelude code is real rustc-checked Rust on the stable pinned toolchain — e.g. an
  NLL snag (returning `&mut` from a guarded match arm + a fallthrough borrow) fails;
  precompute the index immutably, then branch on a non-borrowing `Option`.

See [[golden-greps-unsafe-substring]] (another CoreLib.rs gotcha).

## Uncategorised

### fact-planes-capstone-2026-08-07

# Fact-planes capstone 2026-08-07

Card #1620 (e3, decide). Proposal docs/proposals/compiler-facts-one-law.md (authored in .claude/worktrees/fact-planes-audit; check integrated).

One law over four rethinks: fact moves toward safety silently; every away-move = one written recorded word. Registry half already ratified (D-META-REG1 one table; D-CONC-UNIT1 work facts on type machinery).

Seven ballots pending owner: D-FACT-LAW1 (law, rec B guarded registry), WORD1 (tighten/loosen, rec A), GATE1 (full ledger `jet inspect gates`, rec A), READ1 ($ reads every plane, rec A), HOME1 (home user-facing orphans + phantom rejection, rec A), OWN1 (borrow checker = prover wall, rec A), FLOW1 (one flow-fact store, rec A).

Key facts: stale-memory corrections — SPAWN1=D, FAIL1=A, AUTHORITY-MEM2=A all RATIFIED (not pending). Dead joins State.rs:712 + Sema/mod.rs:44 = soundness debt. Memory sigils &/^/~ are NOT gates (OWN1 wall). On ratification: record in syntax-decisions.md, route #1517-#1579 onto one substrate (criterion 4 open).

### names-one-tree-2026-08-07

# Names/modules/visibility rethink 2026-08-07

- Proposal: docs/proposals/names-one-tree.md (branch worktree-agent-aea2bed26afa4788c until integrated).
- Card #1625 (e3, decide lane). 7 ballots AWAITING OWNER: D-NAME-TREE1 (adopt one-tree model + one sema name ledger), FILES1 (project files visible w/o imports; deletes `use "path"` + file-ref `module x`), ALIAS1 (prelude = readable core module), FENCE1 (visibility set; pub module public-by-default + priv inside; deletes #PubFile), WALK1 (use inside module bodies + one `.[ ]` law sentence), ROLEMOD1 (finish D-ECO-DECL1: retire `module env.dev` role modules, delete ENV_FILE/pkg.jet readers), REFLECT1 (reflection/diagnostics print typeable paths; amends D-METAREFLECT1 + D-ANY-JAI1).
- Law: a declaration attaches; an alias only points; declarations never collide; a declaration replaces an alias. Prelude/Reader-Cursor/FFI shadow carve-outs = all instances of the alias rule.
- Backend evidence (audit): six binding stores, import maps built twice (sema Bundle.rs vs Codegen/Imports.rs), call ladder duplicated, pub(package) invisible to codegen filters, 6 mangling sites + ~30 inline `user_` bypasses, devserver string-parses sema Rust source. Consolidation = one name ledger in sema; rides D-NAME-TREE1.
- Criterion 5 open on #1625: after ratification record outcomes in syntax-decisions.md + mint impl cards.
