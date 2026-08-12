# The standing lens

Shared by every Jet audit and research skill. Do not copy this text into a
skill; point at it. One file, many pointers.

The owner should never have to ask for anything on this page. A request for
"lessons", "a report", or "an audit" already includes all of it.

## Why this exists

Jet's goal is to be the last programming language and the best one: the language
any agent would choose for any task in any situation. Analyze against that goal,
not against a summary of whatever you were handed.

## The four questions

Answer all four in every report, whatever the subject is. When a question does
not apply, say so in one line — do not drop it silently.

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

Two consequences worth re-deriving rather than assuming:

- The agent loop is a **closed loop with a machine oracle**, not "a strict type
  system". Judge every language, Jet included, on whether the loop terminates:
  emit → edit → re-run → count drops → zero.
- The training-data problem is a consequence of (a)–(e), not of corpus size. A
  language nobody trained on still works if the compiler teaches per edit,
  in-loop. Treat Jet's unfamiliarity as a design target, never as a fixed tax.

## The micro sweep

Micro findings are not the small half of the work. They are where Jet actually
wins or loses. Sweep every category below each run, not only the ones the
subject dwells on. An empty category is a valid result; skipping one is not.
Each item gets its own row and its own cross-check — never folded into a macro
theme, never dropped for looking too small.

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

- Build the smallest input that should exercise the surface, run the real
  command through `scripts/agent/jet-env`, and read the actual output, exit
  code, and emitted paths.
- Follow the code path from the emitter back to where the value is set. Fields
  that are documented, always empty, hardcoded, or derived by parsing prose are
  invisible to everyone who trusts the spec, and they are common.
- Prefer a live two-case contrast to a claim: one input that works and one that
  should work and does not. That contrast is the finding, in one paste.
- When Jet already has the mechanism, check its **coverage**, not its existence.
  "Jet has X" and "X fires for the cases that matter" are different reports.

## Honesty rules

- **Name where Jet is behind.** A report with no "Jet loses here" section has
  not looked hard enough. Losing on a competitor's strongest axis is the
  finding, and it belongs at the top, not in a footnote.
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

Follow the owner's format: visual-first, tables over prose, example-led, no hard
wrapping, no stuffiness. Lead with the reframe when the popular reading of the
subject is wrong. End with the strongest unverified assumption in the report
when one exists — that line is worth more than another confirmed finding.
