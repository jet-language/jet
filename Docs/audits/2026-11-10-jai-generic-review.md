# D-JAI-GENERIC1 review: ##Pre, a rule that must be decided while compiling

Date: 2026-11-10. Author: BallotLead-3 (Claude Opus). Compiler: a0.0.43 (dev c460ba91e5).
Board: ballot D-JAI-GENERIC1. Earlier pass: BallotFix REPORT.md, section D-JAI-GENERIC1.

## Question

Should `##Pre(condition, message)` require that the rule is decided while compiling, at every call, with
callers keeping their usual arguments and one body serving every value?

## Findings that changed the ballot

1. **The text addressed the owner and cited a card ID.** The situation read "You suggested … We recommend",
   the detail "the form you proposed in q8sdri5o", and both rec whys "the form you proposed" / "your proposed
   form". All now say "the owner's proposed form".
2. **The registry is not an implementation.** Origin/dev `Compiler/Prelude/Markers.jet:9-17` lists marker settings;
   Pre is now declared at `:72-80`, not `:66`. A new setting fits that metadata home but does not implement
   caller evaluation, forwarding or higher-order obligations.
3. **D-META-SPELL1=A is ratified.** Examples use its proposed canonical forms. No regex count is used
   as proof that every form is implemented or absent.
4. **D-PRE-COMPILE1=A is still unbuilt** (ratified 2026-10-31). j05's literal call prints `before` and then
   stops at run time with E3005, at `j05_pre_const.jet:11`.

## Probes (a0.0.43; outputs in probes/out43)

| Probe | Result | Board claim |
|---|---|---|
| j01 `#Pre` on simd_sum, calls with 8 then 6 | prints `10`, then E3005 "lanes must be 4, 8 or 16" (rc 70) | same |
| j02 `##Pre` | E0003 | same |
| j03, j04 number parameter `<lanes: Int>` | E0003 | same |
| j05 literal call | prints `before`, then E3005 | same |
| j06 unbounded `T` with `+` | E0360 MissingOperatorHook | same |

## Peers (sourced)

- Jai: "The #modify compiler directive lets one put a block of code that is executed at compile-time each
  time a call to that procedure is resolved". `#bake_arguments` "creates a new function with the existing
  function's arguments partially evaluated as constants at compile-time".
  <https://github.com/Jai-Community/Jai-Community-Library/wiki/Advanced>,
  <https://github.com/Jai-Community/Jai-Community-Library/wiki/Getting-Started>
- Zig: `@compileError` inside a body with a comptime parameter.
  <https://ziglang.org/documentation/master/#compileError>
- C++: requires-clauses on the signature. <https://en.cppreference.com/w/cpp/language/constraints>

## Adversarial review (per claim)

| Claim | Attack | Verdict |
|---|---|---|
| Opt-in per marker kind | A registry field is only declaration metadata | **Holds within limits**; parser, evaluator and callable checking remain new work |
| One body | The example ignores lanes; generic instances can also share code | **Fails** as a code-size ranking; no candidate measured |
| Wrappers forward | The original outer clause already rejects 16 | **Fails** as an example; explicit propagated obligations and a distinct inner failure are required |
| Spellings agree with D-META-SPELL1=A | Read live ratification; current j probes reject several forms | **Holds within stated limits**; not implementation proof |
| Plain language | Owner addressed as "you" | **Failed**. Fixed |

## Remaining losses

- A adds a new marker mode despite the previous no-new-mark ruling; the amendment is explicit.
- Required-mode dependencies, evaluator failures and callable erasure need complete checking.
- B/D can bind a concrete generic instance; they do not inherently need A's wrapper workaround.
- Runtime dispatch still needs an application-chosen fallback policy.

## Different-family review (GPT, 2026-11-10)

Reviewer: BallotReview7. Read PRE-COMPILE1=A, META-SPELL1=A, CONSTGEN2=A,
META-HASH1=A, PREPOST1=A and COMPTIME-WORD1=A.
The new marker must be an explicit amendment to the earlier no-new-mark rule, not portrayed as already authorized.

| Claim | Attack and independent evidence | Verdict |
|---|---|---|
| Fixed false #Pre already fails during compilation | Reran j05: prints before, then E3005, exit 70. j01 prints 10 then the same stop. | **Fails today**; ratified behavior remains required work |
| The workload computes at lane width 4, 8 or 16 | Read j01/j05: lanes is unused in the body; four inputs are always summed. | **Fails**; label it a contract workload, not SIMD evidence |
| A guarantees one emitted body; B duplicates machine code per value | No candidate or disassembly was measured. Semantic generic instances can share code when meaning permits. | **Fails** as an exact size comparison; remove that disadvantage from B/D |
| A body may use its ordinary lane argument while compiling | Required signature checking does not automatically specialize or stage the runtime body. | **Fails** without a separate parameter contract; body/type uses still require number parameters |
| A wrapper parameter is magically known | A shared runtime body has no one constant value. | **Fails**; carry symbolic call obligations and discharge at outer calls |
| sum_lanes(16) demonstrates the inner rule failing | The original outer rule itself admits only 4 or 8. | **Fails**; use an outer set including 32 and a narrower inner set |
| Every option needs a wrapper for function values | A concretely bound generic instance has no unresolved value parameter. | **Fails**; B/D can bind an instance directly; A must preserve or refuse erased obligations |
| jet fix can always generate runtime dispatch | A fallback changes return/error/effect behavior and cannot be invented from equalities alone. | **Fails**; only preserve an existing complete fallback or show a manual design |
| Unknown input and a false clause are the same error | Truth is not knownness; budgets, evaluator stops and message facts can also be unavailable. | **Fails**; distinct fail-closed diagnostics and no runtime fallback for required mode |
| Runtime contract stripping may remove the required gate | That would silently turn the required contract off. | **Fails**; required gates remain compile-time requirements |
| early: true in a registry establishes one complete mechanism | Source gives a metadata home only. Forwarding, purity, evaluation, user markers and callable boundaries still need implementation. | **Holds within limits**, not current proof |
| Pre is at line 66 | Read origin/dev with exact numbered source; Pre is at 72-80. | **Fails**; citation corrected |
| C must blindly accept ##Post | A uniform required mode can infer applicability and reject unproved result facts. | **Fails** as framing; C now has a complete applicability rule and names its wider scope |
| D is invisible to tools | Hover/docs can render body validation. | **Fails**; only the authored source signature lacks the predicate |
| Ordinary calls require hidden knownness inference | Explicit #run parameter syntax can reuse the canonical value-parameter mechanism. | **Fails** as an exhaustive choice; added E |
| A wins by being the owner's suggestion | A suggestion is not ratification, and PRE-COMPILE1 previously rejected this mark. | **Fails** as authority framing; alternatives argued on merits |

All six j probes were rerun independently; outputs are `probes/review7-j*.run.txt`.
j02/j03/j04 report E0003; j06 reports E0360. None executes A-E.
Read origin/dev `Compiler/Prelude/Markers.jet`, including the exact Pre declaration at 72-80.
Read Jai's community #modify example and currying note, Zig's @compileError reference and C++ constraint substitution rules.
No Jai, Zig or C++ compiler comparison was executed, and no candidate compile-time or binary-size result is claimed.

Recommendation: A is defensible with the repaired semantics, but reviewer prefers existing B's explicit reuse of the ratified value-parameter mechanism.
Added E is the strongest ordinary-call alternative when that call spelling matters; it is new syntax, not current behavior.
Do not post a one-machine-body guarantee, a per-value binary penalty for B, the incorrect wrapper example,
automatic invented fallback code, unqualified tool invisibility or a SIMD performance claim.
