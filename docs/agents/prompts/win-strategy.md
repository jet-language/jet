# Prompt: Jet market-win strategy doc (doc only)

Copy-paste target: run this in a fresh thread. Read this whole file before acting.

## Mission

Write the strategy for Jet winning against languages that already have established trust. Two halves, both critical, one doc:

1. **Product superiority.** Why a working engineer picks Jet over the incumbent for whatever they are doing — concrete features, performance, and simplicity wins per domain, grounded in measured evidence, with honest losses named.
2. **Market takeover.** How a language with zero years in production overtakes languages with decades of it: the trust substitute, the sequencing, what we build and publish, and how we know it is working.

Deliverable is **a doc only** — no Tower epic, no cards, no ballots from this thread. You implement nothing and card nothing; if the doc surfaces work that needs the board, list it in a final "candidate cards" appendix for the owner to hand to the planning agent.

## Ratified decisions (owner, 2026-08-28 session — these are the strategy's fixed points, do not re-ask)

1. **Humans first, always. Broad targeting.** Jet is marketed to human engineers as the best of all worlds — power, efficiency, simplicity — across every domain. We do NOT market Jet as an agentic/AI language, ever. Jet's power, efficiency, and simplicity make it ideal for AI agents as a consequence; that stays a consequence, not a pitch. The doc's voice, examples, and positioning must all be human-engineer-first.
2. **Trust ladder: public living proof → interop → flagship.** The substitute for decades of production is continuously published, reproducible evidence: the gauntlet scoreboard vs C/Rust/Go/Python/Node, the tier-parity conformance dashboard, the fuzzing record, the defect ledger with closure proof. Nobody else publishes this; it converts "new language" from liability to audit trail. Ecosystem interop is the second rung, flagship software the third. The doc sequences all three.
3. **Advertise the Rust backend until self-hosting.** "Memory safety through a proven backend" answers the number-one objection to a young compiler — miscompilation. The claim retires at self-host. (I2 still hides rustc from users at the diagnostics level; this is positioning, not error surfaces.)
4. **Self-hosting is a named pillar with a milestone.** The compiler becomes the flagship program, proves Jet at scale, and unlocks strict performance wins over Rust. The doc defines entry criteria (language stability, hardening gate passed), not a date.
5. **Onboarding is out of scope.** The owner is not worried about tutorials, installers, or first-five-minutes polish right now. The doc covers features and functionality superiority and market mechanics only.

## Evidence to ground it (search first, smallest slice; verify before citing — the board and corpus move daily)

- docs/audits/gauntlet-2026-08-27.md and gauntlet-2026-08-28.html — measured wins AND losses vs C, Rust, Zig, Go, Python, Node: build speed, RSS, latency, LOC/tokens, agent authoring cost. The honest-losses section of the strategy doc comes from here.
- The win-everything campaign (Tower epic #2268, milestones #2269–#2272) — the ratified tier law (AOT races C/Rust/Zig/Go; JIT `jet run` must beat CPython AND Node substantially; first-result latency races everyone), the Rust exception (meet-or-beat while transpiling), the brevity doctrine (≤1.2× Python LOC, beat Python on tokens, readability veto). The strategy doc narrates these commitments outward; re-derive current campaign state before citing it.
- docs/spec/philosophy.md — the dual-facet identity (beginner magic, expert control) is the differentiation story; the strategy must sell it as "simplicity without a ceiling", not as two products.
- .agents/skills/lessons-learned — why peer languages won or stalled (adoption history: Go's simplicity wedge, Rust's safety story, Python's ecosystem gravity, and the graveyard). Mine for the takeover mechanics chapter.
- The hardening-rig design (docs/proposals/hardening-rig.md, if it exists by the time this runs) — the triple gate and continuous rig ARE the "public living proof" machinery; cite them as the proof supply chain rather than reinventing them.

## What the doc must contain

1. **Exsum first** — the whole argument on one screen: the wedge, the trust substitute, the sequencing, the falsifiable success measures.
2. **The superiority case, domain by domain.** A table per domain (CLI/scripting, services, systems, data, web) with the incumbent, Jet's measured edge, and real side-by-side code — the same program in Jet vs the incumbent, where Jet's version is visibly better. Use real gauntlet entries; never fabricate numbers. Where the measurement does not exist yet, mark it "unmeasured" honestly.
3. **Where we lose today.** A first-class section, not a footnote. Every current loss, its owning campaign card, and the credible path to flipping it. This section IS the trust strategy — publishing losses with closure receipts is what incumbents cannot copy.
4. **The trust engine.** How the proof ladder works mechanically: what gets published, how often, how a skeptic reproduces any number themselves, and how the defect ledger turns bugs into credibility instead of shame.
5. **Takeover sequencing.** Phases with entry/exit criteria: proof supremacy → interop reach → flagship (self-hosted compiler) → broad claim. What we say and refuse to say at each phase. Include the positioning discipline: humans-first language, agent excellence never pitched.
6. **Falsifiable success measures.** For each phase, the numbers that would prove it is working or failing — measured, not vanity.

## Doc rules

- Path: `docs/strategy/market-win.md`.
- Owner doc rules: `simple`-skill prose, direct and alive — the owner has rejected both LLM-dense and stodgy-report prose as "absolute shit". No hard-wrapped prose lines (one paragraph per line; the doc viewer renders every newline). Visual-first: tables, side-by-side code, and phase diagrams carry the doc; prose only preps what the reader is about to see. Dark mode for any HTML artifact.
- Ground every number in a named source. Honesty rules from .agents/skills/_shared/standing-lens.md apply: a strategy with no "Jet loses here" section is defective; end with the strongest unverified assumption.

## Working rules

- Thinking and writing are yours. Mechanical evidence collection (gauntlet result extraction, lessons-learned mining, adoption-history research) goes to Luna max subagents or scouts; web research via search where adoption history needs sources — cite them.
- Batch any owner decisions through the ask tool with a recommended answer each; look up facts yourself.
- Commit only the doc (and prompt-file cleanup if the owner asks). Never `git add -A`. No `#N` card refs in commit messages.
