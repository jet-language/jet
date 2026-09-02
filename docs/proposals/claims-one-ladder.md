# Claims: one evidence report, two producers, evidence that rises

Status: proposal, 2026-09-01, revised after the review passes; element 2 of `whole-language-frame.md`. Independently adoptable. Ballots: D-CLAIM1 (recommended C), D-GRADE1 (D), D-GRADE-POLICY1 (A). Nothing here is implemented; transcripts marked illustrative do not run today.

## Executive summary

"Is this claim true?" has twelve answers in Jet, each with its own command and report, and no ladder between them. A `#Post` is checked when the code runs. A `#Test` runs an example. A property test generates inputs. `jet fuzz` generates more. `jet prove` runs the ratified evidence set and asks the solver. None of them knows about the others, so the same sentence must be written up to three times to be checked, generated, and proved.

The proposal keeps both ratified commands and their jobs. `jet test` runs examples, doctests, property tests, and, new, the contracts of effect-free functions with inputs generated from their `#Pre`, within a bounded budget. `jet prove` stays the ratified umbrella (D-PROVE-SEM1); with `--lens solver`, the only producer-enabling lens (D-PROVE-LENS1), it adds solver evidence: it proves the implication precondition-implies-postcondition, never the precondition alone. Both write one evidence report in which every claim keeps every record it earned, named by producer and outcome. A claim the solver proves loses its runtime check (ratified erasure). A package may require a floor with a count; lowering the floor is a visible edit. `jet fuzz` folds into `jet test --grade=generated`. The evidence record reuses the ratified ProofReport evidence item (kind, producer, outcome, count, facet; D-PROVE-SEM1); no second record shape is introduced.

Score: mechanisms deleted 2 (`jet fuzz` as a command, six report shapes into one); capabilities kept all including `jet prove`, lenses, and every artifact law; capabilities gained 3 (generated evidence from contracts, one evidence report, a package floor).

| today | proposed | ballots |
|---|---|---|
| twelve answers, six reports, no ladder | one evidence report; `jet test` generates; `jet prove` adds `proved` | D-CLAIM1 |
| separate evidence words and reports | producer plus outcome plus count, kept in parallel | D-GRADE1 |
| no claim floor in `jet` | `policy: .{ claims: .{ min: … } }` | D-GRADE-POLICY1 |

## The problem: twelve coats, six reports

See `whole-language-frame.md`, Question 2. The rows that matter most:

| claim | spelling | today's evidence | can it rise? |
|---|---|---|---|
| contract | `#[Pre(c, "m"), Post(c, "m")]` | runtime check every build (D-PREPOST1) | only by running `jet prove` by hand |
| example | `#Test("n") { assert_eq(a, b) }` | one example | no |
| property | `#Test fn f(p: T)` | generated inputs | no |
| fuzz | `jet fuzz` | generated inputs, separate corpus | separate command |
| proof | `jet prove --lens solver` | solver, lenses as presentation views (D-PROVE-LENS1) | separate artifact |

Lane D: "no observed automatic release/promotion edge from proof artifact to `jet build`." And at commit `8b9933668` the ratified contract example does not run on any tier (E0956, E2201, ICE 101; card #2509).

## The evidence words (D-GRADE1 option D)

| word | meaning | producer | erases the runtime check? |
|---|---|---|---|
| `proved` | the solver discharged precondition-implies-postcondition for all inputs | `jet prove --lens solver` | yes (ratified D-PROVE-SEM1) |
| `generated(n)` | n inputs generated from the precondition passed | `jet test` | no |
| `examples(n)` | n written examples passed | `jet test` | no |
| `checked` | the runtime check is compiled in (every build, D-PREPOST1); no tool has run it | none yet | no |
| `unchecked` | the check was stripped by build policy and no other evidence exists | none | n/a |

Every record is stored as producer, outcome, and count; a claim keeps all of them. The summary shows the strongest rung by one narrow acceptance order used only for package floors: `proved` above `generated(n)` above `examples(n)` above `checked` above `unchecked`. Compiler facts (types, effect inference) are not claims and have no rung. Gates such as `#Unsafe("reason")` are rights instances and are reported by their own kind, never as claim evidence.

## The proposal on the page

Source unchanged (real today):

```jet
#[Pre(cents > 0, "cents must be positive"), Post(result > cents, "fee must be added")]
fn add_fee(cents: Int) Int -> { return cents + 5 }

#[Pre(text.len() > 0, "text must not be empty"), Post(result.len() <= text.len(), "never grows")]
fn trim_prefix(text: String) String -> { return text.after(" ") }

#Test("fee on 100") { assert_eq(add_fee(100), 105) }
```

`text.after(" ")` returns the text after the first space, or the empty text when there is none; both are no longer than the input, so the postcondition holds.

Today: three commands, three outputs.

```text
$ jet test app.jet
$ jet fuzz app.jet
$ jet prove app.jet --lens solver
```

Proposed (illustrative): two commands, one report.

```text
$ jet test app.jet
claims  5 rows   generated 2   examples 1   not generated 0   failing 0
  add_fee.Pre        generation domain     cents > 0 selects inputs; a caller obligation, not a claim about all inputs
  add_fee.Post       generated(1000)       inputs from Pre
  trim_prefix.Pre    generation domain
  trim_prefix.Post   generated(1000)
  fee on 100         examples(1)
$ jet prove app.jet --lens solver
  add_fee.Post       proved                solver: cents > 0 implies cents + 5 > cents; runtime check erased
  trim_prefix.Post   generated(1000)       solver: not provable in the affine fragment; runtime check kept
```

When a generated input fails, the report keeps the smallest input and the next run replays it first:

```text
  trim_prefix.Post   FAILED   input text = "a b c"; result "b c" has 3 characters, text has 5: postcondition holds; reported for illustration only
```

## Eligibility

Default generation runs only for callables whose inferred row is `-[]>`. A contract on an effectful function, a precondition that names another function, or a parameter type without a generator (the ratified E0613 case) is reported as `not generated` with the reason. The generator refuses rather than guesses, so no false `generated` row is possible.

| callable | default generation | report |
|---|---|---|
| effect-free contract | inputs generated from `#Pre` | `generated` |
| effectful contract or precondition naming another function | no | `not generated` with the reason |
| parameter type with no generator | no | `not generated` with the reason |

## Rungs

| rung | who | code | evidence |
|---|---|---|---|
| 0 | beginner | `#Test("fee on 100") { assert_eq(add_fee(100), 105) }` | `examples(1)` |
| 1 | intermediate | `#Post(result > cents, "fee must be added")` on an effect-free function | `checked` at run; `generated(1000)` under `jet test` with no extra text |
| 2 | expert | `#Test fn fee_grows(cents: Int(1..)) { assert(add_fee(cents) > cents) }` | `generated(n)` with your own domain (ratified property form and inline range, D-RANGETYPE1) |
| 3 | expert | `jet prove app.jet --lens solver` | the ratified umbrella adds `proved` rows and erases proved checks; bare `jet prove` runs every non-solver producer and shows the same report |
| 4 | expert, package | `policy: .{ claims: .{ min: generated(1000) } }` | every claim below the floor is a diagnostic naming the command that can raise it or the property test to write; `min: examples(1)` is the visible edit that lowers it |

Rung 0 is untouched by every rung above it.

## Three exits for the auto-generation magic

| exit | spelling |
|---|---|
| see | the report line names the input count and where the smallest failing input was kept |
| write | `#Test fn name(p: T)` with explicit domains |
| refuse | `jet test --grade=examples`; `policy: .{ claims: .{ min: examples(1) } }` states a floor without generation |

## Decisions

### D-CLAIM1 — one evidence report; which command produces what

| option | what |
|---|---|
| A | one command; `jet prove` and `jet fuzz` retire; rejected by the rival review because it silently amends the whole proof slate (D-PROVE-REPLAY1, D-JPROOF1, D-JREPLAY1, D-PROVE-SOLVER1) |
| B | keep the commands; add an evidence word to each report; nothing shared |
| C (recommended) | one evidence report; `jet test` runs up to `generated` for effect-free contracts within a budget; `jet prove` stays the umbrella and, only under `--lens solver`, adds `proved` for the implication; lenses stay views; `jet fuzz` folds into `jet test --grade=generated --iterations=N` |

Amends: D-JPROOF1 additively (the ProofReport gains an evidence summary per claim under a version bump); the `jet fuzz` surface folds into `jet test`. Default budget: 1,000 generated inputs per contract, settable per package.

### D-GRADE1 — evidence words

| option | what |
|---|---|
| A | six words in one total order including `static` and `assumed`; rejected because `static` is compiler provenance, `assumed` hides the gate kind, and generated and example evidence are not totally ordered |
| B | nouns with the same defects |
| C | numbered levels with a legend |
| D (recommended) | producer plus outcome plus count, kept in parallel; one narrow acceptance order for package floors only |

### D-GRADE-POLICY1 — a package floor

| option | spelling |
|---|---|
| A (recommended) | `policy: .{ claims: .{ min: generated(1000) } }` in the ratified typed policy value; tightens only |
| B | a new `claims: .{ require: proved }` block beside `authority`; rejected: a second home for a tighten-only setting |
| C | no field; reports plus a CI threshold outside `jet` |

Amends: the D-PACKAGE-POLICY-SCOPE1 field set with `claims`. No `max` field: a maximum has no tighten-only meaning.

## What stays

| kept | why |
|---|---|
| `#Pre` | generation domain |
| `#Post` | runtime check |
| `#Test("name") { }` | written example |
| `#Test fn name(p: T)` | property form |
| `assert` | example |
| `assert_eq` | written example |
| doctests `// =>` | doctest row |
| `fact` declarations | compiler facts |
| `jet prove` with `--lens`, `--capture`, and `--replay` | ratified umbrella |
| `.jetproof` | proof artifact |
| `.measure` and budgets (measurements, not claims about truth; they keep their own rows) | measurements, not claims |
| coverage as a report column | report column |

## Implementation shape

| phase | work |
|---|---|
| A | evidence records in the ledger's third store, reusing the ratified ProofReport evidence item shape; the test runner and the solver write records; one report renderer shared by `jet test` and `jet prove`; #2509 (contracts fail on every tier) fixed first, since nothing above can be proved until the contract example runs |
| B | `#Pre`-driven input generation for effect-free contracts on the existing property generator, with the eligibility report |
| C | after D-CLAIM1: `jet fuzz` folded; after D-GRADE1: the words in every report and `jet inspect claims --all`; after D-GRADE-POLICY1: the policy field and its diagnostic with a snapshot |

## Where Jet loses today

| peer | what it does better | Jet today |
|---|---|---|
| Dafny | verifies `ensures` from `requires` as an implication in the build step | every piece and no ladder; the contract example does not run at this commit |
| Hypothesis | joins generation, shrinking, and replay in one loop | every piece and no ladder; the contract example does not run at this commit |

## Strongest unverified assumption

That a contract's `#Pre` is enough to drive input generation for most effect-free functions. Where it is not, the claim stays `checked` and the report says why; no false `generated` grade is possible because the generator refuses rather than guesses.

| assumption | how it is proved | where |
|---|---|---|
| a contract's `#Pre` is enough to drive input generation for most effect-free functions | the eligibility report; the generator refuses rather than guesses | card #2502 |
