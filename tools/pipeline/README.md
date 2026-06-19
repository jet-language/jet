# `pipeline` — task pipeline dashboard

A tiny, dependency-free view over the owner workflow:

```
inbox  →  plan  →  ballot  →  ratified  →  implemented
```

It reads only the canonical docs (`docs/plans/owner-todo.md`,
`docs/plans/sidequests/*.md`, `docs/spec/decision-ballots.md`,
`docs/spec/syntax-decisions.md`) and never writes outside
`docs/plans/sidequests/`.

## Use

```sh
nix develop -c node tools/pipeline/pipeline.mjs            # status (default)
nix develop -c node tools/pipeline/pipeline.mjs new <slug> "Title"   # scaffold a plan
```

`status` shows, at a glance: the inbox Next-Tasks, every sidequest plan,
the open ballot decisions (with their recommendation) waiting on the owner,
and a count of ratified decisions. The closing line tells you whether
anything is blocked on an owner decision.

`new <slug> "Title"` drops a templated sidequest plan into
`docs/plans/sidequests/` so an agent (or you) can fill it in.

## Why it exists

The pipeline is markdown-driven (no database): the owner drops tasks in one
inbox, agents lift them into reviewed plans, decisions surface to the ballot
with worked examples, the owner decides, agents implement and a reviewing
agent verifies. This tool is just a read-only lens on that flow — it adds no
state of its own, so it can never drift from the docs that are the source of
truth.
