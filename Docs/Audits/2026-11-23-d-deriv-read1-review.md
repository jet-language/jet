# Adversarial review: D-DERIV-READ1 draft

Date: 2026-11-23. Reviewer: DerivReadReview (a fresh agent; the draft is G1Facts').
Subject: the draft ballot for card #6848 c8, which asks how a developer
reads how the compiler knows a fact from the one derivation relation.
Recommendation under review: one reader, `jet explain <subject> [file]`,
for record subjects with the build-fact writer chain included, plus a
"Known by" hover line. The draft listed this as option D; the repaired
ballot lists it as option A, and this audit uses that letter.

All paths are relative to the repository root.

## Strength 1: one command answers "how is this known" for any record

Peers' best case. Lean's `#print axioms thm` walks one constant back to
every axiom it rests on (Theorem Proving in Lean 4, "Axioms and
Computation", lean-lang.org/theorem_proving_in_lean4). Rocq's
`Print Assumptions` displays all the axioms, parameters and variables one
or more theorems depend on, and `Print All Dependencies` adds the
constants (Rocq reference manual, "Commands", section "Query commands").
Soufflé's `-t explain` prints a minimal-height proof tree for one tuple,
`setdepth` bounds it, `subproof` expands a cut branch, and
`explainnegation` explains why a tuple was not derived
(souffle-lang.github.io/provenance). All three answer by a stable name,
and all three follow the chain to its leaves.

Attack. A answers by name, but its chain stops after one step. Premises
today are hashes, operation tokens or evidence ids, never another
record's subject: `sig:` and `sha256:` digests for definitions
(`Compiler/JetSema/Source/Sema/SemanticIndex.jet:1955-1963`),
`name@source_index` for frames (`Compiler/JetSema/Source/Sema/Scheduling/Frames.jet:353-361`),
evidence ids for runs (`Compiler/JetFoundation/Source/Evidence/EvidenceReport.jet:216-223`).
The names are weaker than the peers' too. A definition record's subject
is `def:<hash>` and its path sits in the claim, such as
`method:app.Point.area` (`SemanticIndex.jet:181-223`). A frame subject
ends in the lambda's byte offset (`Compiler/JetSema/Source/Sema/Scheduling/FrameCalls.jet:52`),
which moves on every edit above it. Declarations and frames are only
known after a check, so the lookup costs about what `jet check` costs,
where build-fact explain only loads the program
(`Compiler/JetCLI/Source/CLI/BuildFactExplain.jet:32-73`). The frame
record, the draft's own example, is built and then dropped: only its
reference is kept (`Compiler/JetSema/Source/Sema/Scheduling/Game.jet:198`).
Nothing like `setdepth` or `explainnegation` is offered.

Verdict: holds within limits. A is one command for every record, but
it is one step deep until c8's producers link premises, frame subjects
are not stable names, and the declaration lookup costs a check. The
repaired ballot states all three as losses with mitigations.

## Strength 2: build-fact chains become records and keep today's text

Peers' best case. Bazel's `aquery` reads every action from one
post-analysis action graph and prints its mnemonic, inputs and outputs,
so one store answers for every artifact (bazel.build/query/aquery). The
draft claimed the same: the build-fact explain becomes a reader of the
relation and "keeps its output".

Attack. The draft's own example showed a new format
(`producer jet-build · rule nearest-writer`), and no such producer or rule
exists. Today's text is pinned by two goldens:
`Tests/Cases/CLIPartsSurface/explain_build_setting.txt` (through
`explain_typed_build_setting_golden.case`) and
`Examples/Metaprogramming/BuildTime/BuildContribution.explain.out`
(through `Tests/Cases/PackageOutputs/computed_build_contribution_records_lock_and_matches_explain_golden.case`).
The writer chain is ordered (declaration, optimization bundle, command
line), but `derivation_record` sorts and deduplicates premises
(`Compiler/JetFoundation/Source/Derivations.jet:44`), so a chain stored
as premises loses its order. The only payload case is `FrameSchedule`
(`Derivations.jet:16-18`). The draft's `--json` replaced today's explain
envelope (code, stage, what, why, fix, example,
`Compiler/JetCLI/Source/CLI/Explain.jet:8-21`). The chain also depends on
`--profile` and `--set` (`BuildFactExplain.jet:32-73`), which a stored
record must carry. The draft's command line `jet explain #build.settings.loud`
loses its subject in a shell, because an unquoted `#` starts a comment;
the case file quotes it.

Verdict: holds only as repaired. The ballot now stores the chain as a new
ordered `DerivationPayload` case beside `FrameSchedule`, keeps the text
byte-identical against both goldens, keeps the build-fact lookup
load-only, keeps the JSON envelope and adds one `derivation` field, and
quotes every `#` subject. None of this is built yet.

## Strength 3: every subject `jet explain` answers today keeps its answer

Peers' best case. Rust's `rustc --explain` and Jet's own explain both
keep one stable lookup that users learn once. The draft claimed the new
subjects meet only "one real overlap", `module.UpperCamel` Diagnostic
types.

Attack. The real order in `jet_cli_explain`
(`Compiler/JetCLI/Source/CLI/Front.jet:2648-2715`) runs: refused or
redirected flags, diagnostic codes and `Jet.` names
(`Explain.jet:28-41`), code-shaped subjects, the words `log`, `effects`,
`strictness` and `marker` (`Front.jet:2673-2684`), build facts
(`Compiler/JetCLI/Source/CLI/SyntaxExplain.jet:21-45`), module values
with a file (`BuildFactExplain.jet:78-117`), policy keys, syntax words,
registered facts (`Compiler/JetCLI/Source/CLI/RegistrationExplain.jet`),
unknown `Jet.` names (`Front.jet:2706-2710`), Diagnostic types
(`Explain.jet:45-62`, `Front.jet:2712`), the syntax typo report, and
last the Jetpack package explain (`Front.jet:2715`,
`Jetpack/CLI/Source/Commands/ExplainCommand.jet:21-71`). Package refs
may be dotted (`Jetpack/Store/Source/Explain/PackageExplain.jet:614-626`),
and any `@` after the first character marks a package ref such as
`name@version` (`Compiler/JetCLI/Source/CLI/SyntaxDictionary.jet:130`).
So `main.on_frame@412` reaches Jetpack today and gets E1274, "no package
record or build attempt" (`ExplainCommand.jet:58`). The draft's rule
that an unmatched subject is an error would have broken
`jet explain <package>`. Codes and declarations do not collide: codes are
upper-case and undotted, and module segments are snake_case
(`Docs/Spec/Reference/core-library.md:195`).

A residual collision remains after the repair: a package ref whose first
segment equals a project module's name now reaches the records instead
of Jetpack.

Verdict: holds within limits. The ballot keeps today's whole order and
adds one step before the package fallback, taken only when the first
segment names a project module. `jetpack explain` stays the explicit
package spelling, which covers the residual collision. The ballot names
the shared shape as a loss.

## Strength 4: no new command, word or editor request

Peers' best case. Bazel keeps `aquery` and `cquery --output=graph`
separate from `build` and gives `aquery` a query language
(`inputs`, `outputs`, `mnemonic`) and output formats
(bazel.build/query/aquery). That separation buys listing and filtering,
which option B (`jet explain derivation <file>`) also offers by listing
every record of a file.

Attack. A has no listing and no query language, so a developer must
already know a subject. A hover line for every record would be noise:
every checked-definition record reads the same (jet-sema,
checked-definition, static derivation; `SemanticIndex.jet:1955-1963`).
The hover itself is cheap, because the LSP's check already runs in Doc
mode and builds the index records (`Compiler/JetCLI/Source/CLI/LSP.jet:30-55`),
but hover shows no record today (`Compiler/JetLSP/Source/LSP/Navigation.jet:31-65`).
Compile-time code already reads the index records through the compiler
value's `derivations` (`Compiler/JetEval/Source/CompilerValueMethods.jet:78`).
B and C split "how is this known" across two readers, because the build
fact keeps its own path; D leaves the terminal with jq over
`jet inspect compiler check` (`Compiler/JetCLI/Source/CLI/InspectCompiler.jet:91-130`),
which does not hold frame records at all.

Verdict: holds within limits. A is the only option with one reader. Its
lack of a listing is a stated loss, mitigated by a miss that lists the
module's closest subjects. The hover line is limited to records that say
more than "checked definition".

## Other draft claims

`jet debug` answers a different question: holds. Its `why <place>`
command, run in a replay, finds the write that gave one runtime value its
contents (`Compiler/JetCLI/Source/CLI/Debug.jet:618-656`,
`Compiler/JetDebug/Source/Native.jet:4879-4894`).

Records describe only what the compiler concluded before the program
ran: fails. Recorded and sampled runs are record methods
(`EvidenceReport.jet:216-223`), used by test outcomes and comparison
history (`TestOutcome.jet:363`, `ComparisonHistory.jet:73`). Card #6848
c8 covers "facts, execution and optimization".

Producer and reader lists: incomplete in the draft, corrected in the
ballot. The evidence-record producer was missing, and EvidenceJSON
(`EvidenceJSON.jet:66-67`) was missing among the readers.

## Verdict

Holds within limits. The recommendation is the right direction: one
reader keeps one answer to "how is this known", as Lean, Rocq and
Soufflé do. The draft could not post as written: "keeps its output",
"one real overlap" and "before the program ran" failed, and the lookup
cost was unstated. The repaired ballot fixes each and validates.

## Open gaps

Nothing is built or probed yet. The new lookup step, the ordered
build-fact payload, the JSON field and the byte-identical text are all
design claims. Mitigation: the two goldens,
`explain_build_setting.txt` and `BuildContribution.explain.out`, must pass
unchanged in the cutover, and c8's own proof
(`jet test Compiler --profile cases --filter Explain/derivation_relation_`)
must cover a declaration, a frame schedule and a build fact.

Chains stop after one step until c8's premise links land. Mitigation: c8
makes producers name premise records, and explain nests a premise as
soon as it names one, with no change to the command.

Frame subjects end in a byte offset that moves on every edit.
Mitigation: hover shows the exact subject to copy, and `main.on_frame`
alone answers when the module has one frame callback.
