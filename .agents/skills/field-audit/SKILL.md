---
name: field-audit
description: >-
  Combined competitive leave/stay and peer-strength gap audit. One report, one
  backlog. Day-zero frame: actionable gaps only, no trust or age findings.
---

# Field Audit

**Day-zero frame.** Judge every language, Jet included, as if all of them
shipped tomorrow with no history. Age, trust, adoption, community size, and
package counts are givens, not findings — never report them as losses or flip
criteria. Compare the shipped artifacts only: language, stdlib, tooling,
packaging, docs. Every finding must name work Jet can do to close it.

**Peers.** If the owner names a language, audit that one. Otherwise cover at
least ten peers across families: systems (Rust, Zig, C++, Odin), managed (Go,
Kotlin, C#, Swift, Java), scripting (Python, TypeScript, Ruby), scientific
(Julia), concurrent (Elixir), close analogs (Nim, Crystal), config (Nix).

In one pass:

1. Per peer: jobs people hire it for, what its artifact does better than Jet's
   today, what Jet's does better, verdict, actionable gaps, flip criteria.
2. Peer strengths Jet lacks, ranked backlog (core → stdlib → tooling →
   packaging → docs), plus footguns Jet already avoids (keep list).

Do not invent shipped Jet features — verify each claimed gap against the tree
before reporting it. Prefer `scripts/agent/jet-env` runs. Write the report with
the `simple` skill prose rules.

## The standing lens

Apply `.agents/skills/_shared/standing-lens.md` in full: the four questions, the
five agent-optimality quantities, the micro sweep, probe the running binary, and
the honesty rules. The owner never has to ask for any of it.

The day-zero frame above and the lens agree: both strip away maturity, adoption,
and history, and compete on the artifact. Where they appear to disagree, the
day-zero frame wins for this skill.

## The beat table

Section 2's ranked backlog answers "what Jet lacks". Add its mirror, answering
"what Jet wins", as a ranked table:

| Vector | Peer evidence | Jet's mechanism | Shipped or designed | What they must change to match |
|---|---|---|---|---|

Rank by how categorical the win is, never by effort. A vector a peer could adopt
next release is worth less than one that would break its own model — say which
each is.

**Mark shipped versus ratified-but-unbuilt on every row.** A design that wins on
paper and does not run is a plan, not an advantage, and reporting it as an
advantage is how a competitive audit starts lying. Verify each row against the
tree the same way section 1 verifies gaps.

**Name where Jet is behind, first.** A field audit with no losing row has not
looked hard enough. Losing on a peer's strongest axis is the most valuable
finding the skill can produce, and it belongs at the top of the report.

## Output

Write one markdown report under `docs/audits/` via the Tower CLI (never hand-edit board JSON):

```
node plugins/tower/tower.mjs docs add --section audits --id <skill>-YYYY-MM-DD --title "…" --file -
```

Or `docs update docs/audits/<skill>-YYYY-MM-DD.md --file -` for the same day only when the owner asks to revise that run.
Never overwrite a different day's note. Do not write reports under `docs/plans/`.

Follow `AGENTS.md`. Pick this skill alone — do not chain other audit/research
skills unless the owner asks.
