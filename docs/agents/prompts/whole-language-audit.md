# Prompt: whole-language first-principles audit (plan/card/ballot only)

Copy-paste target: run this in a fresh thread. Read this whole file, then load the `first-principles-audit` skill and follow it in full — this prompt adds constraints on top of it, never replaces it.

## Mission

Rethink the ENTIRE Jet language from first principles. Two goals, equally binding:

1. **Find the unifying frame.** Scan the whole corpus for similar-shaped items living as separate systems — the same underlying job done twice or five times under different names — and unify them. The score is mechanisms deleted, capabilities kept, capabilities gained, in that order. Less machinery, cheaper maintenance, easier reasoning.
2. **Make Jet legitimately revolutionary.** The owner has seen and rejected gimmicks. The bar for "revolutionary": it makes real code genuinely better or easier to reason about, or genuinely faster, or genuinely rethinks how programs are built. If a candidate idea would not survive a skeptical staff engineer asking "so what does this buy me on a real program?", it is a gimmick — kill it yourself before it reaches the owner.

You produce proposals, cards, and ballots. **You implement nothing.** Another agent runs implementation. Do not write compiler/stdlib code.

## Ratified decisions (owner, 2026-08-28 session — do not re-ask these)

1. **Shape: one whole-language audit for the unifying frame, then per-area proposals.** One research fan-out over everything — syntax, semantics, type system, effects, memory model, tier model, comptime, stdlib shape, tooling surface — hunting same-shaped-separate-systems. Synthesis names the one idea or small axis set. Each unification then lands as its own proposal + ballot slate riding the shared frame, so any subset can be adopted independently.
2. **Frozen walls may be challenged; ballots decide.** No macros, no HKT, no top type, comptime-never-creates-types, and the I-invariants may be questioned when evidence is strong. A challenge must carry worked code, the measured cost of the wall today, and a named amendment in its ballot. Default expectation: walls survive. Never silently contradict ratified law.
3. **The revolution-axes hard gate.** Six candidate directions were tabled: agent-closed-loop language, tier model as live programming, effects/capabilities as the one permission system, data/serialization unification, correctness gradient, deterministic builds + time-travel debugging. The owner is open to ALL of them, but you MAY NOT develop any into a proposal until you (a) describe that direction to him ELI5 — plain words, a concrete tiny example of what a user's day looks like with it, what it costs — and (b) he explicitly agrees. Use the ask tool, one batched checkpoint, one ELI5 block per direction, before Phase 2 synthesis develops any of them. Directions he declines are dropped without residue. Unification work (goal 1) needs no such gate — proceed on evidence.

## Method (first-principles-audit skill governs; these are emphases)

- **Research fan-out is mechanical → Luna max subagents and scouts.** Corpus scans, inventory tables, decision-record extraction, silhouette hunting (phantom types, closed compiler tables, string-smuggled data, spec promises with no code, ratified-but-unbuilt decisions), prior-art mining. You personally do all synthesis and design. Cap lanes near five; give each a bounded non-overlapping slice with exact deliverables; lanes flush incrementally within 10 minutes.
- **Probe the running binary** (.agents/skills/_shared/standing-lens.md). Rebuild `target/debug/jet`, run real programs through `scripts/agent/jet-env` on the mechanisms under study. A spec paragraph is intent, not evidence.
- **The evidence table of shadow systems is the spine.** Every mechanism doing a job that another mechanism also does, with home (file:line) and defect. This proves the rethink is needed and becomes the migration checklist. The "five coats of one law" side-by-side table format is exactly what the owner wants.
- **Surface is the product.** Every unification must show up on the page as before/after Jet code where the after is visibly better. An internals-only rethink is a failed run — the owner has rejected one for exactly this.
- **Beginner magic, expert control — proven, not claimed.** Every element gets the mandatory ladder: rungs from "types nothing" to "controls the authority", each opt-in, real code per rung, no upper rung changing the lowest rung. Every magic default shows its three exits in-line: the command that reveals what it resolved, the explicit spelling that replaces it, the project switch that refuses it. Check ceremony creep and magic-without-an-exit by name.
- **rli5 pass is mandatory.** Before the proposal goes to the owner, run the `rli5` skill (true-beginner profile) over every proposed surface: read the proposed code as a genuine newcomer, attempt explain/predict/modify tasks, and produce the friction table. Fix the findings or justify them; attach the table to the proposal. Beginner-friendliness is verified, not asserted.
- **Existing proposals are input, not competition.** docs/proposals/ already holds structure-program-is-a-value, stored-invariant-facts, yielding-loops, transactional-rollback-regions, ecosystem-shape, streamline-one-repo. The audit either absorbs each into the frame, names it an independent survivor, or proposes retiring it — never leaves an unexplained parallel proposal standing.
- **Honesty rules.** Name where Jet loses. End with the strongest unverified assumption. Kill-check every slice: hollowed beginner defaults, invariant carve-outs without ballot, duplicated mechanisms, harder machine repair → kill or narrow before the owner sees it.

## Deliverables

1. Frame document at `docs/proposals/whole-language-frame.md` per skill Phase 3: exsum first; the shadow-systems evidence table; the one idea stated in one sentence; the axes; the law; the final-vision spread (complete programs, today vs proposed, structure tree). Owner doc rules: `simple` prose, direct and alive; NO hard-wrapped prose lines (one paragraph per line); visual-first — every claim gets a code block, table, or tree; dark-mode for any HTML.
2. Per-area proposal docs (`docs/proposals/<area>-<slug>.md`) for each unification, each independently adoptable.
3. Tower: one homed card carrying the audit with exit criteria; full ballot slate per the tower-ballot skill — direction-level, genuine alternatives, worked code per option, amendments named inside ballot text. Validator traps: adversarial pass max ~2 sentences, 32-word sentence cap, never `??` in prose. Ballot first; never ask permission to mint.
4. Per-finding disposition table from .agents/skills/_shared/audit-dispositions.md in the retained doc.
5. A memory note (learn tool): the slate exists, what awaits the owner.

## Working rules

- Batch owner decisions through the ask tool with a recommended answer each; look up facts yourself.
- Re-derive board and repo state before trusting any prior-session fact; the board moves daily.
- A syntax-area sweep includes the OPEN lexical space (prefix/suffix conventions, reserved namespaces, sigils, casing) — propose a use or state a reservation; consolidating existing spellings alone is an incomplete run.
- Commit only owned paths (proposals, Tower store via CLI). Never `git add -A`. No `#N` refs in commit messages. Never run `tower serve`. /tmp is RAM-backed — scratch goes to `~/.cache`.
