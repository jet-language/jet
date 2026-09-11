# Prompt: design Jet's systematic hardening rig (plan/card/ballot only)

Copy-paste target: run this in a fresh thread. Read this whole file before acting.

## Mission

Design the machinery that systematically protects Jet against bugs, broken features, and silent wrong answers. Most of Jet's code is AI-generated: assume a higher defect rate and lower baseline quality than a human-written compiler. The end state: the owner can hand Jet to professional software engineers and they hit no issues. Onboarding/docs polish is explicitly OUT of scope — this is about features and functionality working correctly.

You produce a design doc, Tower cards, and ballots. **You implement nothing.** Another agent runs the implementation burndown. Do not write compiler/stdlib/tooling code, do not run burndowns, do not close implementation cards.

## Ratified decisions (owner, 2026-08-28 session — do not re-ask these)

1. **The handoff gate is a triple gate.** Jet is professional-ready when ALL hold: (a) a registry-driven conformance corpus is green on every execution tier — every Core API exercised by value-consuming programs, with a denominator law so no surface is invisible; (b) sustained differential fuzzing (Jet vs oracle, tier vs tier) over a defined window with zero silent-data findings; (c) a fresh-context adversarial red-team session fails to produce a P0; plus (d) zero known P0s on the board.
2. **First investment: differential oracles + conformance corpus.** Every P0 class found so far is silent-data (wrong values, not crashes) across tier seams and Core-surface gaps. Crashes are easy; wrong answers need an oracle. Grammar fuzzing, property-based testing, and mutation testing are later layers — sequence them in the plan, but the differential+conformance layer is built first.
3. **Cadence: continuous background rig with auto-carding.** A detached local runner cycles differential fuzz + conformance on this machine between sessions and mints deduplicated Tower cards per find. Structural rigor without workflow bloat: no merge gating, no manual sweeps as the primary mechanism.
4. **Default-tier zero tolerance** is standing owner decree (docs/agents/agent-memory.md): any default `jet run` divergence is P0 by default.

## Context to load (search first, smallest slice)

- docs/audits/silent-data-sweep-2026-08-28.md — the defect landscape this rig must make impossible. Read fully.
- docs/audits/datetime-and-tier-hardening-2026-08-28.md — prior structural-hardening thinking.
- Tower cards #2285 (core-call registry claims never reconciled with real ambient arms), #2286 (registry-driven conformance corpus with denominator law), #2287 (Display/Debug shape×context golden matrix + admission ratchet). Your design absorbs and extends these — never duplicates them. Re-derive their current state from the board first; they may have moved.
- The existing proof machinery: dev_corpus_gate three-tier byte-diff, golden-tested examples (I5), UI snapshots (I4), milestone composed sweeps, `scripts/agent/proof-parallel.sh`. Map what exists before adding anything.
- AGENTS.md invariants, especially I9 (tier parity) and I3 (sema checks, codegen dumb).

## Known failure modes the design must defeat by name

- **Coverage lies.** `CoreCallRecord::new` hardcoded `CoreCallCoverage::ALL` while ambient arms were missing — the registry claimed coverage it did not have. The rig must derive denominators from ground truth (registry rows, dispatcher arms), never from self-declared flags.
- **False-green probes.** Bind-and-discard programs pass while the value-consuming form fails (the uuid.v4 lesson). Every generated conformance program must consume its result observably.
- **Invisible surface.** Anything with no example is invisible to example-driven gates. The denominator law: every Core module item appears in the corpus or is named on an explicit, owner-visible exclusion list with a reason.
- **Tier seams.** Interpreter-vs-AOT-vs-JIT divergence is where the worst bugs lived (semantic equality, indexed-place lowering, packed-Int tags, release emission totality). Tier-vs-tier byte-diff is an oracle that needs no external reference — exploit it hard.
- **Resource blowups.** /tmp is RAM-backed tmpfs; `target/` once hit 619G; a continuous runner that leaks disk or RAM will OOM the machine. The design must state its resource budget, its scratch location (`~/.cache/…`, bounded), its pruning rule, and how it respects `scripts/agent/tmp-guard.sh` and `JET_TARGET_CAP_GB`.

## Design questions the doc must answer (with a decided recommendation each)

1. **Oracle sources.** For each Core domain: tier-vs-tier self-differential, reference-language oracle (Python/Rust equivalent programs), algebraic laws (roundtrip, ordering, identity), or golden human-blessed outputs. A table: domain × oracle type × why.
2. **Program generation.** Registry-driven templates per Core API row, corpus mutation of existing examples, grammar-based generation for language constructs — what generates what, and how generated programs stay value-consuming.
3. **The runner.** Process shape (detached, bounded cycles), scheduling, crash recovery, where results live, how a cycle's verdict is recorded reproducibly (seed, jet commit, program, expected, actual).
4. **Auto-carding.** Dedup key design (root-cause seam, not symptom text), P0 default for default-tier divergence, how a find links its reproducer, how fixed cards get a regression lock (the reproducer joins a permanent corpus).
5. **The red-team protocol.** A repeatable adversarial session recipe: fresh-context agents, defined attack surfaces (tier seams, extremes, release vs dev, stdin/argv/env, resource limits), scoring, and what "failed to produce a P0" measures.
6. **The gate dashboard.** One command or artifact that answers "how far from handoff-ready are we" with real numbers: conformance denominator coverage, fuzz window state, red-team status, open P0 count.
7. **Layering roadmap.** When grammar fuzzing, property-based stdlib laws, and compiler mutation testing land relative to the first layer, and what each additionally catches.

## Deliverables

1. Design doc at `docs/proposals/hardening-rig.md`. Owner doc rules: exsum first; `simple` skill prose, direct and alive, no stodge; no hard-wrapped prose lines (one paragraph per line — the doc viewer renders every newline); visual-first — tables, trees, worked examples carry the doc; every mechanism shown, not described.
2. Tower cards for every implementation slice, each with robust observable exit criteria and a dumb-executable plan, homed in the correct epoch. Cross-link #2285/#2286/#2287: extend or absorb, never duplicate.
3. Ballots (tower-ballot skill, full profile) for anything owner-gated. Validator traps: adversarial pass max ~2 sentences, 32-word sentence cap, never use `??` in prose.
4. A memory note (learn tool) recording the slate and what awaits the owner.

## Working rules

- Thinking and design are yours. Mechanical evidence collection (corpus scans, probing the binary, inventorying registry rows vs dispatcher arms) goes to Luna max subagents or scouts.
- Probe the running binary per .agents/skills/_shared/standing-lens.md — rebuild `target/debug/jet` first, run through `scripts/agent/jet-env`. Intent (spec, card, registry row) is not evidence; only execution is.
- Batch owner decisions through the ask tool with a recommended answer per question. Look up facts yourself; only decisions go to the owner.
- Commit only owned paths (the doc, Tower store via CLI). Never `git add -A`. No `#N` card refs in commit messages (githook trap). Never run `tower serve`.
