# Failure marks review (D-FAIL-VISIBLE2)

Date: 2026-11-25. Reviewer: ReviewFailVisible3 (Claude Opus, fresh agent).
Author: RedraftFailVisible2 (Claude Opus). Card #7034.

Subject: the ballot that makes every call able to pass a failure up read
`call()?`, with `jet fmt`, `jet fix` and the editor writing the marks through
the checked pass that D-CALLSITE-MARK3=A builds. Option B keeps today's rule
(D-FAIL-VISIBLE1=A); option C requires marks only in library code.

Sources: dev e3bf920e00a for code, the installed jet a0.0.46 (built from
c137227f924) for probes, and the Tower decisions D-ALLOC-FAIL1 and
D-FAIL-VISIBLE1. The scratch directory was deleted after the run.

## Claims and verdicts

### 1. About 27,000 calls gain a mark

Method: a per-file compiled count with a0.0.46 on the trees it can check, and
a static fixed-point count on every tree at dev 72bd6f6904c, calibrated on the
compiled rows.

Checks run:

- The static script on a fresh `git archive` of 72bd6f6904c gave 26,901
  certain sites with the exact per-tree figures in the ballot (Compiler 9,323,
  Core 2,933, Jetpack 7,818, Tools 2,734, Examples 921, Tests 3,172).
- The compiled script on 18 files not chosen by the author (every 90th
  Examples file and every 150th Tools file at c137227f924): all checked, 260
  marks, 5 receiver edits, no file with other errors. The static script,
  calibrated on those rows, predicted 254 (precision 0.976, recall 0.954).
- Arithmetic in both tables, the per-1,000-line rates and the pooled 0.753 and
  0.747 hold.

The small sample is dominated by one Tools file (225 marks in
Tools/CanvasTest/DebugScenarios.jet), so it confirms the method, not the
per-tree rates. Compiler, Core and Jetpack stay uncalibrated, as the ballot
says.

Verdict: holds within stated limits. About 27,000 is an honest order of
magnitude; the ballot already marks the Compiler figure as possibly off by
half.

### 2. A refused allocation owes no mark

D-ALLOC-FAIL1=C (Tower: "Recoverable by default; explicit stop") makes a
plain `rows.push(n)` fail with `AllocError`. Docs/Spec/spec.md:3865-3880
still describes the older abort default, so the decision is ratified but not
built, as the ballot says. Without the exemption nearly every list and string
operation would need a mark, so the exemption is needed. The Swift rationale
(swift/docs/ErrorHandlingRationale.md, "Marked propagation") says such a
language cannot claim fully marked propagation, and the ballot names that
limit.

Defects found and repaired:

- The rule did not say what happens to a mark written on an allocation-only
  call. The stale-mark warning, as worded, would let fmt delete a mark that
  shows a real exit. Repaired: such a mark is accepted and never stale; the
  warning fires only where a call cannot fail at all.
- Two mitigations said Swift "makes the same exception". Swift aborts on
  allocation failure; its note states a limit, not an exception. Reworded.

Verdict: holds within stated limits, after repair.

### 3. `loop?` and `{x:Hex(2)?}` are the only new spellings, with registry rows

- The loop header's `next` is built with `AST_NODE_GENERATED`
  (Compiler/JetSema/Source/Sema/Statements/ForIn.jet:284), and
  Compiler/JetSema/Source/Sema/FailureMarks.jet:124 skips generated nodes, so
  a failing step is an unmarked exit today. `KW_LOOP` sits at
  Compiler/JetFoundation/Data/Syntax/CoreSurface.jet:503-505. Confirmed.
- The selector is a call root that propagates
  (Compiler/JetSema/Source/Sema/Expressions.jet:2993-2999), and the parser
  rule ends at the `)` with no room for a mark
  (Compiler/JetParser/Source/Parser/Expressions.jet:1450-1482). The cutover
  named "the format selector row" without a site. The row is
  `INTERPOLATION_SELECTOR_RAIL` with its comment
  (Compiler/JetFoundation/Data/Syntax/EffectsSurface.jet:173-181); repaired.
- No other construct needs a spelling: `sema_expression_propagating_call`
  (Compiler/JetSema/Source/Sema/Expressions/Control.jet:526-537) is false for
  operators, indexing and fields, and `sema_expression_auto_propagate` has
  three callers: the general expression path, a match subject and the
  selector (the loop header's `next` takes the general path). Added to the
  ballot.
- Compiler/JetFoundation/Data/Syntax/Spellings.jet holds `LANGUAGE_REWRITES` at line 112, as cited, with
  `mark-caret1` as its latest row.

Verdict: holds.

### 4. Generated calls owe no mark through a generated-origin bit

Confirmed: CLIEntry.jet:165 parses the `#CLI` wrapper, and only items with
an `external_type` get `compiler_generated` (CLIEntry.jet:174-176).
Control.jet:1440-1444 shows why `compiler_generated` cannot be reused.

Defect: the ballot set the bit in `parse_generated_source`, but that entry
point also parses embedded Core
(Compiler/JetParser/Source/Parser/Entry.jet:164-170). A bit set there would
excuse every Core call, about 2,900 estimated marks, and would make Core the
one tree whose exits stay unmarked. JetIR already names the generated
functions in `ir_generated_fragment_function`
(Compiler/JetIR/Source/BuildExpr.jet:7115-7119). Repaired: that test moves
to one shared place that JetIR, E0434 and `jet review` read, and
`parse_generated_source` gains no bit.

Verdict: failed as drafted; holds after repair.

### 5. A stale mark warns, and fmt deletes it

Confirmed: today a mark on a call that cannot fail is accepted. The rule is
sound for single-target code, and `?(text)` is kept because it holds written
words.

Gap found: a callee can fail on one target and not another through `#Target`
impls or `#if #build.target.os` (spec.md:1451-1471). If marks followed one
machine's check, fmt on two hosts would add and delete the same mark.
Repaired: a call owes a mark when its callee can fail on any target the
package builds for, and a mark is stale only when it fails on none. Whether
the failure solve joins targets today was not checked; the ballot marks it
[inference] and the cutover adds a two-target test.

Verdict: holds within stated limits, after repair.

### 6. Decisions amended

The list covers D-FAIL-VISIBLE1, D-FAILURE-FOUNDATION1, S7 and D-FAIL-CTX1,
D-ITERATOR-FAIL1, D-FORMAT-TRAIT1, D-ALLOC-FAIL1 and D-CALLSITE-MARK3
(fmt's insert-only sentence and its `--explicit-failures` note). The
D-ALLOC-FAIL1 sentence now also says a written mark there stays. No missing
decision was found.

Verdict: holds.

### 7. Option fairness and plain language

B keeps today's rule with its unbuilt tools finished, and its costs are
stated. Its one real advantage under the owner's rule was missing from the
summary: a callee that newly can fail breaks no caller's build. Added. C is
the call-mark ballot's library option, set aside there for the same reasons.
The lint reports no plain-language gaps.

Verdict: holds after repair.

## Gaps

1. Whether the failure solve joins every target: open, for the build card's
   two-target test.
2. Compiler, Core and Jetpack counts are uncalibrated; the ballot states this.
3. Residue in fences and templates that do not check is counted only when the
   `fail-mark2` row is built, as the ballot says.

## Result

Recommendation A holds under the owner's rule, with the repairs above. The
ballot passes the lint with both gap lists empty.
