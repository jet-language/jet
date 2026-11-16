# Jet refounding: ordinary code, live results, checked boundaries

**First-principles product audit · 5 September 2026 · Main/Astra**

[Visual report](index.html) · [Twelve published ballots and finding owners](06-decisions-and-finding-dispositions.md) · [Board publication receipt](publication.json) · [Verification receipt](verification.json)

## The recommendation

**Make a Jet program carry enough meaning that the compiler can do the repetitive work around it.** A function should become a checked server endpoint without a second handwritten description. A query should keep its types, work on a file or a changing table, and update an answer without a second handwritten update algorithm. A game coordinate should say which space it belongs to. A test should control time without adding a fake-clock parameter to every function.

That is a product direction, not a proposal for another registry, framework, or configuration language. The implementation should reuse Jet's types, ownership, effects, ordinary functions, generated declarations, and shared executable representation. The new power belongs in those existing concepts. The repeated adapters, descriptions, and private decision paths should disappear.

The earlier report concentrated on proving and explaining Jet. That work matters, and its six follow-on ballots are now ratified A. It did not adequately answer what Jet should *become*. This report makes that missing product argument, with concrete APIs, programs, alternatives, removals, and implementation boundaries.

## What would actually change?

| Product change | The ordinary task it improves | The advanced capability it unlocks | What the user stops writing |
|---|---|---|---|
| **F01 · One endpoint declaration** | Expose a typed function as an HTTP operation | Checked generated clients, request binding, schema and contract comparison | Parallel request parsers, client declarations, and API descriptions |
| **F02 · Typed queries** | Filter, join, and total records without losing their types | One typed plan for collection, file, SQL, and columnar execution | String conversions for keys and numeric conversions just to satisfy an aggregation API |
| **F03 · Live queries over changing data** | Keep a leaderboard or dashboard current | Incremental maintenance of eligible queries, with bounded retained state | A second algorithm that manually keeps the answer synchronized |
| **F04 · Space-safe geometry** | Convert a mouse position into a world position | Checked transform composition across world, screen, camera, and device frames | Comments saying which coordinates a vector happens to contain |
| **F05 · Deterministic execution worlds** | Test a timeout or cancellation without sleeping | Controlled time, scheduling, and declared external inputs through the same effect boundaries | Fake-clock plumbing, global overrides, and timing guesses |
| **F06 · Resource schedules** | Run a frame's rendering and compute work safely | Checked resource hazards, lifetime-based reuse, and inspectable scheduling | Hand-maintained read/write dependency lists and barriers that duplicate program knowledge |
| **F07 · Typed columnar interchange** | Use data produced by another tool without a row-copy detour | Checked Arrow buffer sharing and one typed file-query path | Pairwise Python/R/engine adapters with different lifetime and conversion rules |
| **F08 · Models as typed packages** | Load and use a model as an application dependency | Checked tensor interfaces, pinned tokenizer/weight relationships, bounded streaming inference | Ad hoc shape dictionaries and separately versioned model assets |
| **F09 · Run the project, not a shell ritual** | Run a checked-out Jet project with one command | Exact environment realization delegated to Jetpack, with visible trust and offline controls | Entering a shell solely to make `jet run` find its own declared environment |
| **F10 · Generate hostile histories** | Find “cancel, close, then late callback” bugs | State-aware generation and shrinking through the existing test/evidence commands | One-off event-sequence harnesses that know nothing about the declared protocol |
| **F11 · One complete Core declaration** | Add an API without adding a new compiler dialect | Generated checking and marshalling coverage from the adopted MIR/Core contract | Separate public-name, signature, type, and backend registration edits |
| **F12 · A source-first program workbench** | See why a value changed and what an edit affects | Live dependency, ownership, state, and execution views over checked facts | Reconstructing relationships from unrelated logs and raw records |

F11 and the foundation of F12 are already ratified directions, not discoveries. Several other rows extend existing decisions rather than replace them. Each chapter separates the existing contract from the proposed increment. Nothing in this table claims a shipped implementation.

## Read the audit in the order that answers your question

| If you want to know… | Read |
|---|---|
| Why these changes belong together, rather than being a feature shopping list | [First principles and the evidence](01-first-principles.md) |
| What the language and everyday APIs would look like | [Language and Core design](02-language-and-core.md) |
| How the difficult runtime, compiler, package, and tool pieces would work | [Tools and systems](03-tools-and-systems.md) |
| Whether this helps build an actual application | [Eight complete workload walkthroughs](04-domain-walkthroughs.md) |
| Which ideas come from research, what might be genuinely new, and how correctness is established | [Research, correctness, and release](05-research-correctness-and-release.md) |
| Which owner choices, implementation work, and defect shapes follow | [Decisions and finding dispositions](06-decisions-and-finding-dispositions.md) |

The detailed chapters are intentionally long. Their headings state the decision or lesson. The code comparisons carry the argument; the explanations define the concepts before using them.

## The language should get more capable without becoming harder to start

A beginner should still start here:

```jet
fn run() {
    print("hello")
}
```

They should not need a project graph, a proof assistant, a scheduler, an effect provider, a model server, or an explanation database to print a line.

The next useful step should introduce one idea because the job needs it:

| Job | First concept needed | Advanced concepts that stay out of the way |
|---|---|---|
| Read a small file | A fallible library call | Streaming, mappings, quotas, foreign buffers |
| Group a list of records | A typed query | Physical plans, indexes, incremental maintenance |
| Keep that result current | An explicitly changing source and a watched query | Change propagation rules and retained indexes |
| Draw a button | A UI node and an event handler | Host adapters, accessibility transport, render scheduling |
| Test a timeout | A controlled execution world | Scheduler choices, replay identities, protocol exploration |
| Run a project | `jet run` | Package realization and build-action sandboxes |

**Ordinary bindings do not become reactive. Ordinary lists do not become lazy. Ordinary code does not start a distributed system.** Advanced behavior remains an explicit next step, with a normal result and a visible failure mode.

## The end-state picture

```text
The programmer writes ordinary Jet
  types + functions + ownership + effects + explicit library operations
                               |
                     one checked program
                               |
              the meaning is retained, not re-guessed
                  /            |             \
        generated boundary   typed query    executable MIR
        clients and codecs   operations     and source locations
                  |            |             |
          HTTP / foreign   batch / live   run / build / debug
          application      same result    same program meaning
                  \            |             /
                      source-first workbench
                 values, causes, costs, and failures
```

This is not one enormous runtime object. A type declaration, a checked function, a live query, and a saved execution have different lifetimes. Jet already ratified that distinction in D-LEDGER1. The proposal shares *definitions and typed interfaces*, not every storage table, lock, or scheduling algorithm.

## The removal budget is part of the design

An addition earns its place only if it removes repeated work or prevents a named class of mistakes.

| Remove or retire | Keep instead |
|---|---|
| A second handwritten schema for an exported function's ordinary typed inputs and outputs | A checked transport mapping over the function and type declarations |
| A string-and-Float aggregate result where the input's key and amount types were known | A generic aggregate result retaining those types |
| A hand-maintained incremental algorithm beside an eligible ordinary query | A checked transformation of the same query |
| Separate stale-publication rules invented by each new reactive feature | The existing revision/lifecycle authority with one publication check |
| A fake-time parameter threaded through unrelated business functions | A scoped provider at the existing effect boundary |
| Private Core-name/type/signature lists for a new public API | The complete declaration and generated projections required by the MIR/Core ruling |
| Cache control that cannot account for one of the compiler's durable cache roots | The already-ratified machine store and honest per-kind accounting |
| Tests whose only claim is that a field was copied or a wrapper forwarded a call | A consumer-visible behavior test, or no permanent test |

Do **not** delete differences that mean something. A read-only view and an owning value are not duplicates. A one-shot result and a changing result are not the same lifetime. A database transaction and a build action do not become the same operation because both can be drawn as a graph.

## What is fixed, and what needs a choice?

**Fixed authority:** I1–I9; safe beginner defaults; explicit expert control; exact observations across applicable execution tiers; the ratified MIR/Core architecture; the carrier-plus-knowledge type foundation; the adopted compiler-proof requirement; and the six recently ratified proof/tooling ballots.

**Owner choices:** the new public APIs and mechanisms described in the design chapters; the explicit environment-default amendment; any exact external library needed for an expanded format or model backend; and any change to an existing ruling. A proposal does not amend a decision by describing itself confidently.

**Protected records:** D-PLACE1, D-ACCEL1, D-LOOPREAD1, cards #2920–#2923, and frozen card #2420 are not reopened by this audit. Device and acceleration policy remains theirs. No AI assistant, AI mode, or remote model service becomes a requirement of the compiler, editor, learning tools, or package manager. F08 concerns applications that deliberately use a model, not an AI dependency for developing Jet.

## Evidence: what this report can and cannot establish

| Label | Meaning |
|---|---|
| **Ratified** | The live Tower decision or operative specification requires it |
| **Source-observed** | The implementation route, declaration, or repeated work is present in the inspected checkout |
| **Historical execution** | An earlier retained probe ran; its date and old-binary limitation remain attached |
| **Proposed** | This report defines intended behavior and a design choice; it is not runnable Jet today |
| **Experiment** | The linked standalone investigation was actually executed; its conclusion is limited to that model |
| **Unmeasured** | No current same-job Jet/peer measurement establishes the claim |

The fresh-compiler construction failure remains accepted ground truth. It was not retried. This report does not turn source inspection, a board's `done` state, a printed command, or a proposed code sample into proof that the current compiler runs it. In particular, the eight domain batteries have source evidence and retained historical probes, not fresh current-tree execution proof from this campaign.

## The acceptance test for this proposal

A useful replacement must let you answer all of these questions without trusting adjectives:

1. What can I build that was previously awkward or unsupported?
2. What exact code or command do I write?
3. Which repeated code or compiler mechanism disappears?
4. What happens on a wrong type, stale result, exhausted budget, missing provider, or unsupported target?
5. How do I see the automatic choice, replace it explicitly, or refuse it?
6. Which existing decision does this reuse or amend?
7. What would falsify the promised simplification, correctness, or performance benefit?
8. Who owns the complete implementation, including the non-native execution paths?

The chapters answer those questions for each retained feature. The final disposition table accounts for the original thirty research questions and every retained defect shape; it is not a claim that a large list of cards is itself a product.
