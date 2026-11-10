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
2. **The opt-in setting has a concrete home.** `Compiler/Prelude/Markers.jet:9-17` lists the `impl Marker`
   settings (`sites`, `repeatable`, `owns_menu`, `inherits`, `resolution`, `scopes`, `companion`), and `Pre`
   is declared at `:66`. A's `early: true` is one more setting in that list, so the marker registry needs no
   second mechanism.
3. **D-META-SPELL1=A is ratified and unbuilt.** It was ratified on 2026-11-10 ("#run, #if and #loop replace
   the comptime word"). On dev, `git grep` finds no `#run` line and 2,816 `comptime` uses in 1,013 files. The
   ballot's spellings follow the ratified form, and the detail now says it is unbuilt.
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
| Opt-in per marker kind | Does the registry have settings? | Holds: Markers.jet:9-17 |
| One body | Does `##Pre` force a copy? | Holds: a parameter read only by the rule stays an argument |
| Wrappers forward | sum_lanes(16, xs) | Holds: each outer call decides both rules |
| Spellings agree with D-META-SPELL1=A | Is it built? | Ratified, unbuilt. Now stated |
| Plain language | Owner addressed as "you" | **Failed**. Fixed |

## Remaining losses

- Two ways to require a known value, with the error teaching which to use.
- A bare function value needs a wrapper, the same under every option.
