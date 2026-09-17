# Type-unification traps and reviews

## Ratification and soundness traps

Re-verify every item below in a fresh context. Do not assume an old review is still true.

1. **Fabricated law.** Quote ratified decisions verbatim with line numbers. Comments in reference files are non-normative. One misquote can kill a finding.
2. **S26's forever list.** “Comptime types” are rejected forever. Ordinary enums consumed at compile time need no new category. Say “amends S26,” never “clarifies,” when a proposal touches it.
3. **Same-day verdicts.** `D-UNIFYLIT1=A` was ratified hours before one draft tried to relitigate it. Check ratification-log dates.
4. **Soundness pricing.** Jet v1 has no lifetimes. A proposal that moves a handle across a frame boundary must state its capture and escape rule and price a real mechanism, such as owned captures or second-class parameters.
5. **Compatibility grading.** Jet is greenfield. Surface respells are near-free. Grade retrofit risk on representation (serialized data and snapshots) and habit (idioms accrete), not break-cost arithmetic.
6. **Duplicated authority.** Before typing a handle such as `Capability<E>`, ask what it buys over the ambient mechanism (effect rows plus grants). If v1's memory model forecloses the style it enables, record it as a post-v1 note.

The boundary law remains in the root: control constructs are not types; their runtime artifacts can be. Do not reify a keyword to make a review pass.

## Mandatory fresh-context reviews

On every authorized run, run and record all three passes. A report-only boundary still records the passes but creates no board work.

- **Peer pass:** assume every cite and quote is wrong. Re-run the headline probes, check paths and decision IDs, and correct the report.
- **Adversarial pass:** attack the proposal against invariants, S26, D-EXT1, the beginner bar, soundness, and the type-versus-control boundary. Reject inert checks and accidental new mechanisms.
- **Pay-up-front pass:** ask where the expensive path is actually right and where it is speculation. Price frame capture, representation, compatibility, and migration implications honestly.

Record each pass's material findings, evidence, and resolution in the report. The verifier must have fresh context and must not merely trust the builder's census.

## Permission boundary

Cards and ballots are created only after the owner explicitly authorizes that outcome. Bugs map to cards. Syntax, surface, API, or feature changes map to `tower-ballot`; they do not become implementation work by implication. Use the shared audit-dispositions contract for publication and finding markers.

## Review stop

Close when the target census, live probes, ratification-date checks, soundness and I8 checks, and all three review resolutions are recorded. Remaining unavailable evidence is an explicit `unknown`; it is not a reason to widen the target or invent a type.

## Anti-goals

- Do not reify keywords such as `loop`, `if`, or `taskgroup` as values; use the boundary law.
- Do not add a new meta-type kind when a ratified mechanism can carry the facts (I8).
- Do not propose value-dependent types; literal-only positions stay literal-only.
- Do not relitigate ratified verdicts, including a verdict ratified today.
- Do not propose a check that checks nothing; a fact kind without a consumer is inert magic.
