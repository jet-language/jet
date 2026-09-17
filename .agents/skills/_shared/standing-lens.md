# The standing lens

Shared by every Jet audit and research skill. Do not copy this text into a
skill; point at it. One file, many pointers.

This page owns the shared evidence lens only. Publication, workflow boundaries,
write permissions, and the disposition marker live in
`.agents/skills/_shared/audit-dispositions.md`; load both where the method
requires them.

## Scope and depth

Use [Jet's priorities](../../../docs/spec/philosophy.md) to judge the requested subject. The task defines the domain, sources, workloads, and output; include dependencies needed to understand that subject, not every conceivable adjacent topic.

An explicit full audit covers its entire declared corpus and required categories. A focused question stays focused. State the coverage boundary and account for unavailable evidence; do not silently sample away requested work. Record unrelated opportunities without automatically pursuing them.

Apply the relevant sections below. Competitive questions belong to comparative work; the micro sweep belongs to language/API/UX analysis; live probes establish executable behavior. A skill or document audit does not require unrelated compiler runs. `spec-compliance-audit`, `isomorphic-ontology-audit`, and `type-unification-audit` use the probe and honesty sections, not the competitive frame.

## The four questions

For comparative work, answer these questions where the subject supplies evidence. Mark a required but inapplicable question with a short reason. Do not manufacture a comparison, advantage, or loss to fill a section.

1. **How do we beat this on a level playing field?** Assume equal maturity,
   ecosystem, longevity, and hiring pool. Compete only on technical merit. Name
   the vectors where Jet wins categorically, not incrementally. A vector the
   competitor cannot adopt without breaking its own model is worth more than one
   it could copy next release.
2. **What do we avoid?** Anything that works is also evidence about its own
   mistakes. One row per mistake: the mistake, its evidence, and Jet's exposure.
   Include mistakes Jet is structurally immune to; immunity is a design asset
   worth stating once.
3. **What does this say about AI-driven development?** Reason from the five
   quantities below, not from what a source happens to claim.
4. **What concrete surfaces must Jet cover?** Types, methods, APIs, defaults,
   operators, syntax forms, diagnostics, and commands — named exactly, in three
   groups: covered with proof, worth checking, missing.

## Agent-optimality: the five quantities

The frame for question 3, and the source of most micro findings.

| | Quantity | What it means | Where it is won or lost |
|---|---|---|---|
| a | **Verdict fidelity** | Does the compiler catch the mistake, or does production? | sema coverage (I3), effects, contracts, budgets |
| b | **Verdict latency** | Time from edit to verdict | per-file checkability, incremental queries |
| c | **Verdict actionability** | Can the agent act with no inference? | typed edits, applicability grades, causal chain, blast radius (I4) |
| d | **Context economy** | Tokens per unit of progress | source verbosity **and** diagnostic verbosity |
| e | **Repair determinism** | How many valid fixes one error admits | one-mechanism design (I8) |

For agent-development claims, examine the whole feedback loop: emit, check, repair, and check again. Measure whether the compiler teaches an unfamiliar construct through useful diagnostics; do not infer agent success or failure from training-corpus size alone.

## The micro sweep

Within the declared language/API/UX scope, account for each applicable category below, including clean categories and reasoned not-applicable results. Each concrete finding retains its own evidence; do not hide a small defect inside a macro theme. A full surface audit retains all ten category rows.

- **Syntax** — a spelling that reads well or badly; noise, ceremony, sigils,
  nesting, punctuation that exists only to satisfy the compiler.
- **Ergonomics** — how many steps the obvious thing takes; what must be said
  twice; what must be known before starting.
- **Surfaces** — the shape of a module, namespace, or command area; what is
  reachable, discoverable, or buried.
- **APIs, types, and methods** — exact names and signatures worth having, worth
  avoiding, or missing. Write them down as names.
- **Defaults** — what happens with no configuration; whether the safe thing is
  the default or an annotation. Defaults are where safety claims are really won.
- **Naming** — a word that clarifies or misleads; a term that made a concept
  click; a name that lies about what it does.
- **Error text and diagnostics** — phrasing, structure, what/why/fix quality,
  length, and whether the message is machine-actionable as well as readable.
- **UX and DX** — the loop the developer lives in: watch, run, test, debug,
  iterate. Where waiting happens. Where the tool surprises them.
- **Tooling and CLI shape** — command names, flags, output formats, exit codes,
  machine-readable modes, editor integration.
- **Ceremony versus control** — where the language forces ceremony with no
  payoff, and where it hides control an expert needed.

Harvest praise and complaint with equal care. "I just like how this reads" and
"I hate waiting for this" are both product data, and both usually go
unrecorded. A personal-preference remark is evidence about the surface even when
it is not evidence about the technology.

## Probe the running binary

A spec paragraph, a ratified decision, a Prelude declaration, and a Tower card
are all evidence that someone intended a thing. None is evidence that it works.
The highest-value findings come from running Jet's version of the mechanism
under study.

- For a claim about current executable behavior, rebuild the relevant binary, construct the smallest representative input, and run the real command through `scripts/agent/jet-env`. Read its output, exit code, and emitted paths. Reuse evidence that already proves the same claim against the same source state; do not rerun unrelated checks.
- Follow the code path from the emitter back to where the value is set. Fields
  that are documented, always empty, hardcoded, or derived by parsing prose are
  invisible to everyone who trusts the spec, and they are common.
- Prefer a live two-case contrast to a claim: one input that works and one that
  should work and does not. That contrast is the finding, in one paste.
- When Jet already has the mechanism, check its **coverage**, not its existence.
  "Jet has X" and "X fires for the cases that matter" are different reports.

## Honesty rules

- **Report observed results.** Put evidenced losses and failures first. Zero losses is a valid result when required coverage and measurements are complete. Missing, invalid, or unavailable evidence cannot support a win; apply the strict performance gate when benchmarking.
- **Mark shipped versus ratified-but-unbuilt** on every claimed advantage. Most
  beat-the-field vectors will be designed and unbuilt. "The design already wins;
  the risk is execution" is a legitimate verdict, but only when you checked.
- **Measure carefully or not at all.** Before quoting a coverage number, confirm
  the source holds real data: ledger tables and registries often carry
  placeholder text, and a percentage from the wrong column is worse than none.
  Where a metric cannot be derived honestly, describe the architecture instead.
- **Confirm a surprising source-level finding with a second reader** before it
  reaches the report. Tool output can carry display artifacts.
- **Do not restate the subject's own conclusion as the finding.** Its framing is
  evidence, not the answer. State the real mechanism when the two differ.
- **Difficulty is not a tradeoff.** Rank on Jet's priorities: safety, beginner
  experience, runtime performance, one mechanical path. Never on effort.

## Reporting

Use the owner's requested format. Prefer worked examples and tables when they clarify findings; do not activate HTML merely because the result is a report. Preserve exact source links and technical terms. End with the strongest material unverified assumption when one remains.

The evidence pass is complete when the declared scope and required categories are accounted for, claims have appropriate evidence or an explicit unknown, and findings have the requested recommendations. Publication and authorized board actions follow [audit-dispositions.md](audit-dispositions.md); completing a report never implies implementing its recommendations.
