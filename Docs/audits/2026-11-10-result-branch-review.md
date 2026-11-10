# D-RESULT-BRANCH1 review: deleting the result operator

Date: 2026-11-10. Author: BallotLead-3 (Claude Opus). Compiler: a0.0.43 (dev c460ba91e5).
Board: ballot D-RESULT-BRANCH1 on card #6281. Earlier pass: BallotFix REPORT.md, section D-RESULT-BRANCH1.

## Question

Should the operator `r ? n -> … ! e -> …` go, leaving the arm table, the single-arm if and `??`?
It was ratified as D-RESULT-DECON2=B on 2026-08-24, so A reverses a ratified choice.

## Findings that changed the ballot

1. **The migration broke a ratified rule.** The ballot's A said "a registered retirement error; jet fix
   rewrites it". D-TEACH-RECONCILE1=A (ratified 2026-11-05) says retired Jet spellings get no teaching
   message and no jet fix rewrite. A now does a single parse-tree rewrite of every repository use in the
   commit that deletes the form. After that commit the old form gets the generic parse error.
2. **The cost of 135 longer handlers was too high.** D-FAIL-BIND1=A (ratified 2026-08-06) makes `err`
   ambient inside a `??` fallback. If a handler uses the error and its success arm is the bare value
   (`? v -> v ! e -> f(e)`), it becomes `?? f(err)`, still one line. 59 of the 135 qualify, which leaves 76
   that become arm tables. Probe r08 shows the rewrite prints exactly what the operator prints.
3. **`! _ ->` is not legal today.** The operator requires a named failure binding (E0003, "A Result handler
   failure branch needs a payload binding"). The earlier report's example `! _ -> return 0` is written
   `! e -> return 0`. The census already counts an unused name as "ignores the error".

## Probes (a0.0.43; outputs in probes/out43)

| Probe | What it does | Result |
|---|---|---|
| r01_operator | the operator, three uses | `got 3`, `failed Empty`, `size 2` |
| r02_table | the same program with arm tables and `??` | same three lines |
| r03_oneline | single-arm `if r == .Ok(n)` and `.Err(e)` | `got 3`, `failed Empty` |
| r04_table_oneline | an arm table on one line with commas | E0003 |
| r05_semicolon | an arm table with semicolons | E0373 |
| r06_value | one value three ways | `20 20 20` |
| r07_rewrites | `?? return 0`, single-arm if and two-arm table beside the operator forms they replace; the subject prints on each evaluation | each subject prints once; outputs match (`102 102 0 0`, `20 20 -1 -1`, `ok 2`/`bad Empty`) |
| r08_err_fallback | `? v -> v ! e -> return "bad {e}"` against `?? return "bad {err}"` | each subject prints once; `ok 2`, `bad Empty` for both |

## Census (dev c460ba91e5, probes/rb-census-dev.mjs, run from the dev worktree)

```
code: 419 in 155 files; error arm uses its binding 135, ignores it 284; value position 200
  ignored error arms: 116 leave (return/panic/next/break/continue), 168 are a plain value or call
  success arm is the bare binding (n -> n): 178; of those, error arm uses its binding: 59
  by root: Compiler 13, Core 2, Examples 27, Jetpack 187, Tools 2, tests 188
markdown: 3 in 3 files
```

`?? return` already appears on 19,220 lines of dev Jet code (`git grep -c -E '\?\? *return\b' -- '*.jet'`).

Rewrite shapes under A:

| Shape | Count | Rewrite | Lines |
|---|---|---|---|
| error unused, arm is a plain value | 168 | `r ?? value`, or the single-arm if when the success reshapes | 1 |
| error unused, arm leaves | 116 | `r ?? return x` (also `next`, `break`, `panic`) | 1 |
| error used, success is the bare value | 59 | `r ?? f(err)` | 1 |
| error used, success reshapes | 76 | two-arm table | 4 |

The script's heuristics are the BallotFix ones. An occurrence is a `? name ->` head followed within 400
characters by `! name ->`, with comment lines blanked. A braced error arm is read to its closing brace.
The "leaves" class only checks the first word of the arm.

## Code cited (dev worktree)

- `Compiler/JetParser/Source/Parser/ExpressionForms/ResultHandlers.jet`, 115 lines, deleted under A.
- Other references: 15 Compiler files, with the most in `Compiler/JetParser/Source/Formatter/Expressions.jet`
  (36 lines) and `Compiler/JetParser/Tests/Integration/ResultHandlerGrammar.jet` (16).
- `Examples/ErrorsAndEffects/Errors/ResultHandler.jet` and its `.out` are rewritten.
- `Docs/spec/syntax-decisions.md:2034-2042`: D-FAIL-BIND1=A, `err` is ambient inside a fallible fallback.
- D-ERR-DECON1=A (`syntax-decisions.md:2052`): a `??` block ends in one value or one diverging tail,
  so `?? return …` is the ratified form.

## Peers (sourced)

- Rust uses one `match` for every enum. Arms are separated by commas, and a short match can sit on one line.
  <https://doc.rust-lang.org/reference/expressions/match-expr.html>
- Kotlin: `Result.fold(onSuccess, onFailure)` is an `inline` function. Its lambdas can therefore `return`
  from the enclosing function (non-local return). In Jet a closure return leaves only the closure, so D
  would not get this.
  <https://kotlinlang.org/api/core/kotlin-stdlib/kotlin/fold.html>,
  <https://kotlinlang.org/docs/inline-functions.html#returns>
- Zig: `if (r) |n| … else |e| …` captures either payload of an error union.
  <https://ziglang.org/documentation/master/#if>
- Swift switches over `Result`, one `case` per line, and `if case .success(let n) = r` tests one case.
  <https://developer.apple.com/documentation/swift/result>

## Adversarial review (per claim)

| Claim | Attack | Verdict |
|---|---|---|
| Every use has an exact rewrite | A bare `??` would drop the `return` in an arm that leaves | Holds after the fix: such arms become `?? return x`; r07 |
| The rewrite evaluates the subject once | `??` or the single-arm if might evaluate twice | Holds: r07 and r08 print the subject once per call |
| jet fix does the migration | D-TEACH-RECONCILE1=A forbids a rewrite rule for retired spellings | **Failed; fixed**: one in-tree rewrite in the deleting commit, then the generic error |
| 135 handlers grow three lines | `err` is ambient in `??` (D-FAIL-BIND1=A) | **Failed; fixed**: 59 stay one line, 76 grow |
| `err` means the same as the operator's `e` | It might be a wrapped report | Holds for the probed domain error: r08 prints `bad Empty` for both. The migration checks for an `err` already in scope before using it |
| Seven in ten stay one line | Is the heuristic census reliable? | Now 343 of 419 (82%), from a regex census; any case it misclassifies still has a correct rewrite, the two-arm table |
| C helps every enum | It also gives every arm table two layouts | Holds; this is the reason against it |
| D adds no grammar | Kotlin's fold handles return well | Fair to Kotlin because its fold is inline; Jet has no inline non-local return, so the cost stands for Jet |
| The migration is small | Most uses sit in Compiler | Wrong direction: 375 of 419 are in Jetpack and tests, and only 13 in Compiler |

## Remaining loss

76 handlers grow from one line to four, about 230 lines in all. The old form gets the generic parse error
rather than a message naming the new form; that is the ratified D-TEACH-RECONCILE1=A rule.

## Different-family review (GPT, 2026-11-10)

| Claim | Attack and independent evidence | Verdict |
|---|---|---|
| Existing branch forms agree | Reran r01-r08: r01/r02 print got 3, failed Empty, size 2; r03 binds payloads; r04 is E0003; r05 E0373; r06 prints 20 20 20; r07/r08 agree. | Holds for these examples on a0.0.43. |
| Deletion leaves ? and ! one meaning each | The draft itself lists optional types, fallback, diagnostic forms, negation and error contracts. | Fails literally; corrected to removing one result-specific surface. |
| Regex census proves exact cutover and line cost | Read rb-census-dev.mjs: its 400-character regex window and first-word exit heuristic are not an AST walk. | Fails; exact line-growth forecasts withdrawn, census treated as an estimate. |
| Every rewrite is equivalent | Scalar outputs check one evaluation, not move-only ownership, cleanup, branch laziness, nested err shadowing or loop-target scope. | Holds only within tested limits; full-table fallback required for unproved compact rewrites. |
| C's two layouts violate one mechanism | AGENTS I8 expressly allows helpful spelling and organization differences; C keeps ordinary patterns. | Fails; C's real costs are separator grammar and formatter rules. |
| ?? covers any value-position fold | fold can transform both branches; fallback keeps the success value. Read Kotlin Result.fold, which is inline. | Fails; corrected D's best case. Non-local return is not thereby granted to Jet. |
| The operator was not previously approved | Read D-RESULT-DECON2: outcome B; D-FAIL-BIND1: A; D-TEACH-RECONCILE1: A. | Fails as a possible framing; A explicitly reverses approved syntax. |

Recommendation: A is defensible for a strictly vertical style, but I prefer C for short exhaustive two-payload handlers. It removes the specialized operator without losing compactness or adding a second semantic mechanism. The draft still recommends A, now with the real tradeoff rather than an I8 objection. No new option was needed. Do not post exact regex-based migration guarantees, a speed claim, or the literal one-meaning slogan.
