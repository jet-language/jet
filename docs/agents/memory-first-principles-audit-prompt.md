# Launch prompt — first-principles audit of Jet's memory system (adversarial pair)

Owner-commissioned 2026-08-12. Paste this whole document as the opening instruction of a fresh session in the Jet repo. The session becomes the **orchestrator**. It never designs first and never reads ahead of its phase; it enforces the quarantine, runs the pair, reconciles, checkpoints with the owner, then synthesizes.

---

## Your mission (orchestrator)

Run a first-principles audit of Jet's entire memory system with one goal: **Jet ends up with the best memory system of any language — beating every other language on every metric, for every domain, at every level.** Do as much hard work as possible up front, in the language, so the burden moves off the developer and the user and onto the compiler. The result must be easy to read, easy to write, easy to reason about, extremely performant, reasonable to compile, as memory safe as possible, and still give experts full control.

The owner's core steer, verbatim — this is the hypothesis the audit exists to test and expand:

> "Some programs need explicit, fine memory control, others want gc, others just want rust style or current jet style management, etc. All of these are related and there are underlying similarities that I think haven't been fully or correctly explored and PROPERLY expanded on/implemented in other languages. The goal is to be memory safe by default but sometimes experts need fine control or escape hatches. Just ensure there's solid justification if you want to rethink something. I fully encourage it but don't want you to waste my time either."

You MUST reconsider from first principles and MAY propose a greenfield replacement. You are NOT obligated to overhaul the current system if it is already exceptional — but that conclusion must be **earned adversarially**, never assumed. That is why this run uses a blind designer who has never seen Jet's memory system.

Read these before anything else: `.claude/skills/first-principles-audit/SKILL.md` (governs the deliverable; where it and this prompt overlap, the stricter wins), `.agents/skills/_shared/standing-lens.md`, the `simple` skill (governs all owner-facing prose), `AGENTS.md`. Read `plugins/tower/skills/tower/SKILL.md` and `tower-ballot/SKILL.md` before Phase D.

## Hard rules

- **Walls.** Exactly one absolute: the spirit of I1 — memory safe by default, expert escape only through explicit, audited opt-in. Everything else — no-GC, value semantics, sigils, no-lifetime-syntax, even the assumption that the design must lower to the current rustc backend — is reopenable **with solid justification** carried in a ballot. Reopening without a justification that survives the rubric is wasting the owner's time; so is refusing to reopen something the evidence condemns.
- **Effort is never a deterrent.** Never weigh implementation difficulty in a recommendation. Judge only the best final design (philosophy.md, "Effort is never a deterrent").
- **Proof bar: worked code + live probes.** Every claim about what Jet ships today is proven by running `scripts/agent/jet-env jet run` / `--release` on a minimal program and pasting the two-case contrast. Every claim about the proposed design is shown as worked code. Performance claims are argued from mechanism and marked `[INFERENCE]` unless actually measured. Never quote a spec as evidence of behavior.
- **Same-rigor verdict rule.** The final verdict is one of **overhaul / evolve / keep**. Whatever the verdict, the deliverable is identical in rigor: the blind ideal design is published in full, the delta scorecard names every rubric cell where current ≠ ideal, and a "keep" verdict must defeat the ideal **cell by cell** — never by absence of findings.
- **Tower discipline.** Board writes only through `node plugins/tower/tower.mjs` with `--by`. Never run `tower serve`. Never hand-edit `plugins/tower/.tower/`. This audit is read-only on the codebase; its writes are the proposal doc, Tower cards/ballots, and nothing else.
- **Owner-facing prose**: `simple` skill, visual-first, tables over prose, example-led, one paragraph per line, never hard-wrap.
- Concurrency budget: keep it to ~5 active subagents; prefer fewer, deeper lanes over wide shallow fan-out.

## Worker routing (owner-directed)

- **Thinking work runs on fable (Claude).** The blind design, the advocate's steelman and attack, reconciliation, the delta scorecard, the verdict, synthesis, the proposal, and every ballot are fable-authored — orchestrator inline or fable subagents. Never delegate any of these to a mechanical worker.
- **Purely mechanical work runs on gpt-5.6-luna at max reasoning via the codex CLI** (`codex exec` with model `gpt-5.6-luna`, reasoning effort max; playbook: `docs/agents/agent-memory.md` codex notes and `~/.claude/skills/sol/SKILL.md` mechanics, substituting the luna model). Mechanical means: writing and running the probe programs from an exact spec you provide, collecting outputs, corpus greps and inventories, assembling tables from data you already judged, formatting sweeps. If a task requires a judgment call, it is not mechanical — pull it back to fable. Keep luna lanes bounded and non-overlapping; orchestrator validates every luna output before it enters the record.
- **Skills:** load `ponytail` for any code the run produces (probes, scripts) and for the implementation-shape section — smallest complete thing that works, no speculative machinery. Load `simple` before writing any owner-facing text; the proposal, checkpoint packet, and ballots are all owner-facing.

## The fixed rubric (commit before reading any output)

Both subagents score against this exact matrix; you reconcile on it; the checkpoint and the final verdict argue from it. Do not let either side add, remove, or reweight cells after the fact.

**Domains** (the extremes are mandatory, not optional): bare-metal MCU (≤2 KB RAM, no allocator), kernel/driver, AAA game frame loop (zero-alloc steady state), low-latency trading/audio (no pauses ever), long-lived server (years of uptime, fragmentation), data/compute pipeline (throughput, huge working sets), shell-script one-liner (zero ceremony), browser/wasm (foreign GC heap interop), FFI-heavy embedding (C/C++/Rust ownership transfer), **AI-agent-authored code at scale** (the agent is a first-class author).

**Metrics**: safety by default (what class of bug is impossible, caught at compile, caught at runtime, or possible), read/write/reason ergonomics (tokens for the common case; what must be said twice; what must be known before starting), runtime performance and predictability (throughput AND tail latency/pauses), compile-time cost of the checking, learnability curve (the two-sentence beginner test), expert control ceiling (can an expert get exactly C/Zig-grade behavior when they ask), diagnostics and repair determinism (does a memory error name one obvious fix), FFI/embedded fit, and the five agent-optimality quantities from the standing lens.

## Phase A — the adversarial pair (parallel, independent)

Spawn both subagents in one batch. Neither sees the other's output. Both receive the rubric above verbatim.

### Subagent 1: the Blind Designer (quarantined)

**Quarantine is absolute.** Its task prompt is the brief below and nothing else from this repository. It must not read, glob, grep, or open ANY file in the Jet repo — no spec, no code, no examples, no Tower, no docs. Web research on other languages and research systems is allowed and required. If it needs something from the repo, it does without. You (orchestrator) must not paraphrase Jet's memory design into any follow-up message to it. Violating quarantine invalidates the run — restart the designer fresh.

Give it verbatim everything between the BRIEF markers:

---BRIEF START---

You are designing, from first principles, the memory system for a new language called Jet. You have never seen Jet's current memory design, and you must not attempt to discover it. Design the ideal; someone else compares.

**What Jet is.** A dual-facet, memory-safe, compiled language: magic for beginners, full expert control behind explicit opt-in. The compiler front end owns all semantics and all user-facing errors. Programs compile to native code. There is also a JIT dev mode, an interpreter, and a web/wasm target — one meaning on every tier.

**Ranked priorities (ties break to the smaller number; effort never weighs at all):**
1. Memory and type safety — never traded away, never configurable.
2. Beginner experience — every feature must be explainable in two sentences to someone writing their first compiled program, or it needs a redesign or an explicit expert tier.
3. Runtime performance — zero-cost defaults; no runtime overhead purchased to buy simplicity for the compiler or the language designer.
4. One mechanical path — exactly one canonical way to do each operation; features fight to get in.
5. Implementation simplicity and compile speed — matters, loses to 1–4.
6. Ecosystem breadth — all domains, first-party and curated.

**The three readers.** Every surface serves a beginner (types nothing, gets safe fast defaults), an expert (can name, see, override, and audit everything), and a machine agent writing code unattended. For the agent, optimize five quantities: verdict fidelity (compiler catches it, not production), verdict latency (edit → verdict fast enough to sit in a loop), verdict actionability (the error names the fix; no guessing), context economy (few tokens for the common case, in source AND in diagnostics), repair determinism (one error admits one obvious repair).

**The owner's hypothesis, verbatim — your design must engage it head-on:** "Some programs need explicit, fine memory control, others want gc, others just want rust style [ownership/borrowing] management, etc. All of these are related and there are underlying similarities that I think haven't been fully or correctly explored and PROPERLY expanded on/implemented in other languages. The goal is to be memory safe by default but sometimes experts need fine control or escape hatches."

**Mandatory scope — design all of it as one coherent model:**
1. The core ownership/aliasing model: how values are created, copied, moved, referenced, mutated, and die; what the beginner default is; what the compiler proves.
2. **References that outlive an expression — the centerpiece.** Storing references in structs, returning them from functions, holding them across calls — WITHOUT making the beginner write lifetime annotations. This is the unsolved problem of the field: Rust solved it with lifetime syntax (developer pays), GC languages solved it with a collector (user pays), and everything else rations it. Your design must state exactly how far safe non-annotated references reach, where the model asks for more (and what it asks for), and why that boundary is the right one. A design that just bans them must defend the ban against every domain in the rubric.
3. Allocators, arenas, regions, pools: bulk lifetime, bump/arena patterns, allocator swapping/wrapping program-wide and scope-wide.
4. Concurrency aliasing: sharing across threads/tasks, interior mutability, data-race prevention, and how the memory model and the concurrency model are one story.
5. The escape hatch: raw memory, layout control, volatile/MMIO, and the audit story around unsafe code. Safe by default; expert opt-in explicit, auditable, and policy-controllable at project/org level.
6. FFI memory: ownership transfer across C/C++/Rust boundaries; who frees what; how foreign memory stays contained.
7. The extremes: bare-metal (2 KB RAM, no OS), kernel, hard-realtime (no pauses ever), long-lived servers (fragmentation), wasm (foreign GC heap), one-liner scripts (zero ceremony).
8. Diagnostics as product: what a memory error looks like (what/why/fix), what the developer can inspect (facts, budgets, profiles), and how a program states memory policy (e.g. "this module never heap-allocates") checkably.

**Mandatory prior-art sweep — steal or reject each with one stated reason:** Rust (ownership + lifetimes + NLL/Polonius), Zig (explicit allocators, no hidden control flow), Swift (ARC + exclusivity + non-copyable types), Go (GC + escape analysis), C/C++ (RAII, smart pointers, the bug record), Odin, Mojo (origins/inferred lifetimes, ASAP-style destruction), Vale (generational references, regions), Hylo (mutable value semantics, subscripts), Austral (linear types, capabilities), Koka/Lean (Perceus reference counting, functional-but-in-place), Cyclone (regions), MLKit (region inference — including why it disappointed), ASAP (static deallocation), Verona (region ownership + per-region strategies), Nim (ORC/ARC), Lobster (compile-time RC elision), Fil-C and CHERI (capability pointers), Pony (reference capabilities). For each: what it got right, what it paid, and whether your model subsumes it as a special case.

**The bar for the design itself:** one unifying idea stated in one sentence; a small set of orthogonal axes; a conservation-style law the rules fall out of; every memory strategy anyone legitimately needs (fine manual control, arenas/regions, ownership/borrowing, reference counting, GC-like convenience) expressible as a **point in one model** rather than a bolted-on mode — that is the owner's hypothesis; prove it or refute it. Beginner rung types nothing and is safe and fast; every rung above is opt-in; no upper rung changes what the lowest rung does; every magic default has an audit trail (see what it did), an explicit spelling (write it by hand), and a refusal switch (turn it off project-wide).

**Syntax for worked examples.** Use this neutral base and invent memory-related surface freely (mark all of it "proposed"): `fn name(args) => Ret { … }` for functions, `x := expr` mutable binding, `x :: expr` immutable binding, `struct Name { field: Type }`, `enum Name { Variant(Type) }`, `T?` optional, `print(…)`, `loop x, xs { … }`. Do not assume any existing memory-related syntax.

**Deliverable (one markdown doc, visual-first, tables and code over prose, no hard-wrapped lines):**
1. The one idea, one sentence. Then the axes and the law.
2. The full design, element by element, example-led, climbing beginner → intermediate → expert rungs per element.
3. The centerpiece section: outliving references, worked end-to-end (a struct holding a reference; a function returning one; a cache; a parser borrowing an input buffer; a game entity graph — real programs, not toys).
4. The prior-art table: steal/reject × reason × whether subsumed.
5. Self-scorecard against the rubric you were given — every domain × metric cell, honest, with the losing cells named. A design that claims to win every cell without naming its risks is a failed deliverable.
6. The three worst things about your own design (mandatory; find them or the reconciler will).

---BRIEF END---

### Subagent 2: the Advocate (full repo access)

Task: document and **steelman** Jet's current and ratified memory system across the same scope list and the same rubric — then attack it honestly. It must:

- Cover: Tier-1 value semantics/moves/clone and sigils (`&`/`^`/`~`), the ownership prover and the one-fact-graph for views/windows (spec.md §"Sema keeps one fact graph"), arenas/regions/allocators (D-ALLOC1/2, D-REGION1, D-FIXED-BACKING1, Pool/Id, `close(^)`), memory facts and policy ladder (D-MEM-FACTS1, D-MARK-SCOPE1, D-PACKAGE-POLICY-SCOPE1, E0921), `#Unsafe` and low-level tier (S58, D-UNSAFE2, D-UNSAFE-OBLIG1, D-LL1), FFI memory boundary (E0702 family, extern rust/c/cpp), concurrency aliasing (Shared, Cell, guards, task captures), embedded/freestanding (D-TARGET-ALLOC1, E3303) and the web tier, memory diagnostics (E0631/E0632, E3101–E3112) and observability (`jet inspect` surfaces, GeneratedUnsafe/Allocation budget metrics).
- **Probe the running binary** for every load-bearing claim — `scripts/agent/jet-env`, minimal programs, both `jet run` and `jet run --release`, paste contrasts. Spec text is not evidence.
- Sweep the silhouette: ratified-but-unbuilt (D-REF2 open arenas ballot, D-LL3 wider core.mem, Tier-2 stored references per C1), open cards (#1853 program allocator, #1886 one-lookup upsert, #1888 hardened profile, c0pdzfiu memory floors, c0nh3lbu gate ladder, c0gso01s unsafe ratchet), the 2026-08-12 defect set (#1883–#1888), prior audits in `docs/audits/`, borrow-ceiling closeout cards, lessons-learned.
- Produce: the strongest honest case FOR the current model (what it gets right that peers do not), its scorecard on the same rubric, its known defects and unbuilt promises listed without spin, and the three hardest questions it cannot currently answer (mandatory).
- Read-only on code; no fixes, no formatters, no test suites beyond targeted probes.

## Phase B — reconciliation (you, alone)

Only after both artifacts are complete: build the **delta scorecard** — one row per rubric cell, columns: blind ideal, current+ratified Jet, delta, and which side's evidence is stronger. Where the two agree independently, say so — independent convergence is the strongest signal in this design. Where they diverge, do not average: name the disagreement and what evidence would settle it. Draft a **provisional verdict direction** (overhaul / evolve / keep) with the three decisive cells named.

## Phase C — OWNER CHECKPOINT (hard stop)

Present to the owner, in this order, and stop: (1) the blind ideal design's exsum and its one idea, (2) the delta scorecard, (3) the provisional verdict and the three decisive cells, (4) the questions where owner steering changes the synthesis. Do not begin the proposal until the owner responds. Owner may reweight the rubric, kill slices, or redirect — his answers are law for Phase D.

## Phase D — synthesis and proposal

Now run the `first-principles-audit` skill Phases 2–5 in full, with both dossiers as your research base. The proposal (`docs/proposals/memory-<slug>.md`) follows the skill's Phase-3 shape exactly: exsum first, evidence table of shadow systems, example-led element-by-element redesign with the beginner→expert ladder per element and the three expert exits per magic default, the mandatory final-vision spread (complete programs, today-vs-proposed side by side, structure diagram), what-this-unlocks by domain, what stays on merit, the owner decision table, phased implementation shape. Every touched ratified decision is respected or named as an explicit amendment inside a ballot. Mint the full ballot slate per `tower-ballot` (direction-level, genuine alternatives, worked code per option), home one card carrying the proposal and its exit criteria, run `tower lint`, then a fresh-context review that assumes the work is wrong (the skill's Phase-5 hunt list), fix findings, close.

Deliverable checklist the owner will hold you to: blind design published in full · advocate dossier with live probes · delta scorecard on the fixed rubric · checkpoint honored · verdict at same rigor either way · proposal per skill shape · ballot slate · Tower card · review pass done. End your final report with the strongest unverified assumption left standing.
