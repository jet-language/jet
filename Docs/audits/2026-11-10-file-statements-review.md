# D-FILE-STATEMENTS1 review: a file of top-level statements is a program

Date: 2026-11-10. Author: BallotLead-3 (Claude Opus). Compiler: a0.0.43 (dev c460ba91e5).
Board: ballot D-FILE-STATEMENTS1 on card #6281. Earlier pass: BallotFix REPORT.md, section D-FILE-STATEMENTS1.

## Question

Should a file whose top level holds statements run them in order as its program, with no `run: fn` and
no `#!` line? Dev already does this; Main ruled it ahead of the vote in dev 9d06d0b291.

## Finding that changed the ballot

**`defer` is refused in a top-level body.** The original f19 uses `close(^b)` and fails; it is not the valid run-body comparison.
The different-family section below supplies and runs a valid `defer close(b)` pair.
Top-level defer reports "`defer` is not a Jet word". The cause:

- At the origin/dev snapshot read for this review, `Compiler/JetParser/Source/Parser/Entry.jet:930-937` calls
  `parser_unknown_top_word_ahead` before the ordinary statement parser.
- `Compiler/JetParser/Source/Parser/Statements.jet:1743-1745`: an identifier followed by another word is
  reported as an unknown word. `defer close` matches.
- The block path knows the word: `Statements.jet:113` (`word == "defer"`) and `:528`
  (`parser_parse_defer_statement`).

A's technical text now includes this fix. No in-tree file is affected yet: `git grep -E '^defer '` finds
none at top level, against 101 `defer close(` lines in 52 files inside functions.

Side finding, outside this ballot: the spec's `defer close(^resource)` (syntax-decisions.md:3115) is E0003
even inside `run: fn` ("Expected a value, found `^`"; f19 as committed). No `.jet` file in the repository
uses `close(^`.

## Probes (a0.0.43; outputs in probes/out43, probes/imp/out43, probes/cyc/out43)

| Probe | Claim tested | Result |
|---|---|---|
| f01 `print("hi")` | a one-line file runs | `hi`; check ok |
| f02 `#!` file | the `#!` line is legal | `hi from a script` |
| f03 statements and `run: fn` | the error | E0621 |
| f04 declarations only | a library | check ok; run E0101 |
| f06 / f07 / f08 | tally as body, in run, with `#!` | same three lines each |
| f09 / f10 | ALL_CAPS and `:=` bindings / lower-case `limit: 3` | `limit 3, count 2` / E0357 |
| f11 | a function reads a body local | E0107 |
| f12 | early return | stops after `limit 3` |
| **f14** (new) | a lone binding never makes a program | check ok; run E0101 |
| **f15 / f16** (new) | a body failure matches run | same E3001 message and exit 70; distinct source locations |
| **f17** (new) | `jet test` never runs the body | `double doubles: pass`; `BODY RAN` absent; `jet run` prints it |
| **f18** (new) | forward call to a later function | `hi Ada` |
| **f20** (new) | scope-end cleanup of body locals on early return | `closed 4`, `closed 3`, also observed in review6-defer-run |
| **f13 / f19** (original) | invalid-caret cleanup pair | both fail; use review6-defer-body/run for the valid comparison |
| imp | import of a statement file | E0620 |
| cyc | use cycle | E0604 |

Built binaries: `jet build` of f06 (body) and f07 (run) both give 62,704 bytes with the same output.
121 bytes differ (`cmp -l`). [INFERENCE] The differing bytes are source line and hash metadata. They were
not decoded, since the host has no `strings` or `size`.

## Census (dev c460ba91e5, git grep -c)

- 12,222 `run: fn` lines in 12,189 Jet files. All stay valid.
- E0621 is named 69 times in 23 files and E0620 79 times in 21 files.
- 109 Jet files start with `#!`.
- `TopLevelStatementsBesideRun` appears in 5 files.

## Peers (sourced)

- C# top-level statements are the closest peer to A. "A project can have only one file with top-level
  statements." "The compiler generates a method to serve as the program entry point." "Both styles compile
  to equivalent code." Types come after the statements.
  <https://learn.microsoft.com/en-us/dotnet/csharp/fundamentals/program-structure/top-level-statements>
- Python runs a module's statements on first import, which is option C's model.
  <https://docs.python.org/3/reference/import.html>
- Swift puts top-level code in main.swift, and SE-0343 isolates it to the main actor, which is close to D.
  <https://github.com/apple/swift-evolution/blob/main/proposals/0343-top-level-concurrency.md>
- Go requires `func main` and has no top-level statements. <https://go.dev/ref/spec#Program_execution>

## Adversarial review (per claim)

| Claim | Attack | Verdict |
|---|---|---|
| The body is the run body | Failure, cleanup or lowering may differ | Holds for failure (f15/f16), cleanup (f20), build size (f06/f07) and early return (f12). **Failed for `defer`** (f13); the fix is now part of A |
| A binding alone never makes a program | `count := 2` might be classed as a statement | Holds: f14 |
| `jet test` never runs a body | Test discovery might load the body | Holds: f17 |
| Imports never run statements | | Holds: imp gives E0620 |
| Every `run: fn` program stays valid | | Holds: 12,222 declarations; f07 runs |
| B is cheap | Dev runs these files today | B means refusing files dev runs now |
| "as you agreed" | It speaks to the owner directly | Reworded to "as the owner agreed" |

## Remaining losses

A stray statement runs when its lone file is run by name; before, that file failed with E0101. A function
cannot read a lower-case body local; E0107 offers a parameter or an ALL_CAPS constant. Both are by design.

## Different-family review (GPT, 2026-11-10)

| Claim | Attack and independently checked evidence | Verdict |
|---|---|---|
| Bare statements work | Reran every f01-f20 file, import and cycle probes on a0.0.43. Bare/shebang/tally bodies run; mixed entry E0621; declaration-only and binding-only run E0101; body-local access E0107; imported body E0620; cycle E0604. | Holds for exercised native paths. |
| Binding-only and declaration-only files check | Ran check on f01, f04 and f14: all pass. | Holds; a binding alone does not select an entry. |
| Test discovery runs no body | Ran `jet test f17_test_body.jet`: one test passes with no BODY RAN; direct run prints BODY RAN. | Holds. |
| Body panic reports are identical | f15 and f16 both exit 70/E3001 with the same message, but source files, lines and indentation differ. | Holds only for code/message/exit semantics, not byte-identical reports. |
| f19 proves defer works in run | f19 contains `close(^b)` and is E0003. New `review6-defer-run.jet` uses valid `defer close(b)` and closes 4 then 3. The otherwise equivalent `review6-defer-body.jet` is E0003 at defer. | Original evidence fails; valid pair now establishes the top-level parser defect. |
| Cited parser location is current | Read origin/dev Entry and Statements: Entry guard is :930-937, Statements recognizes defer at :113/:528 and parses it at :1633; unknown-top-word test is :1743. | Old Entry :942 citation fails for this snapshot; corrected. |
| Equal output and binary size prove complete body equivalence | Twenty examples and equal file length do not prove all control flow, error/cleanup paths or tiers. No new build-size check was run. | Fails as full proof; keep complete parity as an implementation requirement. |
| Files, pipes and one-liners share body rules | Ran -e print, -e value and piped print: hi, 3, piped. | Holds for these forms; final-value echo is a distinct -e host behavior. |
| A means no startup effects | Read D-ENTRY-EXPLICIT2=A, D-SCRIPT-TOPLEVEL1=A and D-FFI-COMPTIME2=A directly. The later foreign-constant ruling keeps a startup fallback. | Fails broadly; A forbids application-body execution on import, not all library initialization. |
| C's global lock makes startup safe | The draft itself admits a foreign cross-thread callback can deadlock. A build listing does not prevent that. | Fails; replaced with per-library state publication, nonblocking in-progress failure and refusal of unproved callback/blocking initializer edges. |
| B is worse because dev accepts A | Existing code cannot decide an owner-only design choice. B's visible script intent is a real strength. | Fails as framing; compare mandatory ceremony, not current implementation inertia. |

Read C# top-level statements, Python import/cache semantics, Swift SE-0343 and Go's specification independently. C# is the closest entry-wrapper precedent; it is not proof of Jet behavior. Existing numeric census is historical and not independently remeasured.

Agree with repaired A. No option added. Its remaining costs—accidental direct execution and binding reclassification—are real. Do not post the f19 caret form as working, byte-identical panic reports, complete tier parity, binary-size proof, or a no-startup-effects claim.
