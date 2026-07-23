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

## Output

Write one Tower scratch note via the Tower CLI (never hand-edit board JSON):

```
node plugins/tower/tower.mjs scratch add --id <skill>-YYYY-MM-DD --title "…" --file -
```

Or `scratch update` for the same id only when the owner asks to revise that run.
Never overwrite a different day's note. Do not write reports under `docs/plans/`.

Follow `AGENTS.md`. Pick this skill alone — do not chain other audit/research
skills unless the owner asks.
