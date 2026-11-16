# The owner's bar for a mining run

Harvested from the owner's feedback on every mining run from August to November
2026 (chat, ballots and cards). Each line is a correction he had to make more
than once. Read it before capture and again before drafting.

## Look for more of these

- **Everything that can make Jet better, at every scale.** Not one relevant
  thing per source. Constructs, semantics, APIs, Core libraries, diagnostics,
  tools, learning paths, and also systems, subsystems, architecture and
  infrastructure: build, packaging, runtime, compiler pipeline, memory,
  concurrency, IO, deployment, debugging, observability. A long technical source
  that yields a handful of findings has been skimmed.
- **Capability parity, then beyond it.** When a source shows what another
  language can do, list every capability it shows. For each one, state whether
  Jet does it today, and back that with a probe program. Then design how Jet
  does it better, in breadth and depth. "Can Jet do everything Jai does in this
  video?" is answered by that table, never by prose.
- **Defects, by running the demos.** Rebuild each demo the source shows as a
  small Jet program and run it on the default tier. A wrong answer, an internal
  compiler error or a missing API is a defect finding with its program attached.
  The owner treats any such gap as unacceptable, so find them all.
- **Ways to beat every alternative.** Where the source is the best of its kind,
  name what Jet must do to beat it on every metric that matters to its users
  (speed, memory, size, ease, safety, portability), and how.
- **Simplification.** A finding that lets Jet remove a concept, merge two
  mechanisms or flatten a type is worth as much as a new feature. Challenge a
  convention only when the replacement is genuinely better, and say why.
- **The owner's own comments.** When the owner supplies a comments file or
  questions, each one gets its own row and a direct answer.

## Look for fewer of these

- **A parallel mechanism.** A proposal that adds something beside an existing
  Jet mechanism instead of growing it. Before proposing anything new, name the
  closest existing mechanism and show why extending it cannot do the job. The
  coverage gate enforces this with `extends` or `why_new` on every new claim.
- **Niche features.** Jet ships the tools, glue and libraries that let people
  build niches. It does not ship the niche itself. The owner once erased more
  than forty niche ballots.
- **First-party adapters for third-party products** (message queues, caches,
  hosted services). Those projects provide their own Jet support. Where Jet
  needs a capability, the answer is a Jet Core library, not a binding to a C
  library.
- **Ballots for ordinary API design.** A new Core API that follows idiomatic
  Jet API patterns is a card, not a ballot. Ballots are for owner-only choices
  (AGENTS.md "Owner gates"): syntax, semantics, commands, dependencies, product
  behavior.
- **Padding downsides.** Downsides that are obvious, or that no design could
  avoid, waste the owner's time. Name them once as the job's own cost, not as an
  option's loss.
- **Another round instead of distilling.** More than a thousand claims with few
  outcomes is a failed run. Every claim ends in exactly one outcome: a criterion
  on an existing card, a new card, a ballot, evidence on an existing record, a
  rejection with its reason, or a named gap. `check_coverage.mjs --audit` counts
  and enforces them.
- **Praise and per-language tours.** A confirmation counts only with a
  concrete mechanism and witness. Summaries of what a language is like count
  for nothing.

## Ballots the owner can vote on

- **No real downsides.** Engineer every downside away, creatively if needed,
  and keep redesigning. A recommendation that still lists losses is not ready.
  An irremovable loss is named with its cost and its best mitigation. The
  adversarial review in AGENTS.md comes first.
- **Cohesive and idiomatic.** Read every open and ratified ballot in the same
  area first. The new ballot must agree with them, extend what they decided, and
  read like Jet. The owner rejects ballots that are inconsistent with each other
  or that miss the larger point.
- **Readable at a glance.** The opening explains the situation and why it
  matters in plain words (the eli5 skill), not a list of related things. The
  options show the same tiny program, with current Jet first, then the best
  example from elsewhere, then each option.
- **Current, planned and in-flight Jet.** Every option is written against
  ratified-unbuilt decisions and work in progress, not only the code on disk.
- **Complete.** Every proposal that needs an owner choice gets its ballot.
  A question asked anywhere else on Tower does not count.

## The report

- **Takeaways first.** For each source, a short summary and its specific
  takeaways, in plain words, before any table. The owner once could not open a
  60,000-line report; the evidence lives in the run directory and is linked,
  not pasted.
- **A new-ideas section.** Every new feature, API, surface, syntax,
  architecture, tool and concept, each with its outcome.
- **An order of work.** Priorities and dependency chains, so whoever builds it
  can go straight through.
- **Opus does the thinking.** Helpers only capture transcripts, pages and data.
  Reading, ledgers, claims, designs and ballots belong to the thinking model.
