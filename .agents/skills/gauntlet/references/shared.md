# Gauntlet shared laws

Read this file for both routes. Read `.agents/skills/_shared/audit-dispositions.md`
before activation. That file owns shared publication and workflow boundaries;
the rules below remain gauntlet-owned.

## Day-zero frame

Judge every language, including Jet, as if it shipped tomorrow with no history.
Age, trust, adoption, community size, and package counts are givens, not
findings. Compare shipped artifacts only. Every loss names work Jet can do to
close it.

## Authorship and level playing field

Every headline implementation—Jet, every port, and every fixture—is authored by
a Luna max worker. Use `.agents/skills/orchestration/SKILL.md` for dispatch.
Use the same author and reasoning budget on both sides. Record authoring cost in
`entry.json`: worker turns, retries, and diagnostics hit.

An expert tier is optional and applies only to performance entries. When an
established expert implementation exists, such as a benchmarks-game or real OSS
implementation, check it in as a labeled sourced reference row. Pair it with a
Jet expert variant authored by Sol high. Compare expert with expert, never Luna
with expert. The orchestrator does not author corpus code. Workers do not run
the harness; they type-check with `scripts/agent/lane-check.sh` only.

## Environment

Run everything through `scripts/agent/jet-env`. Follow `AGENTS.md` target-dir
laws: never use `/tmp` targets, use the shared main `target/`, and respect cap
checks. A missing competitor toolchain is an owner-visible flake change, not a
silent skip.

## Standing lens

Apply `.agents/skills/_shared/standing-lens.md` to the route's relevant
questions and evidence. For a full competitive run or corpus review, apply the
full four-question, five-quantity, micro-sweep, running-binary, and honesty
passes. Where the lens and the day-zero frame disagree, the day-zero frame wins.
