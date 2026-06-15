# Jet docs — where to look

One home per kind of document. Start here.

| If you want to… | Go to |
|---|---|
| Learn the language | [guide/](guide/) — the learner's guide, plus the [15-minute tour](guide/tour.md) |
| Look up the standard library | [reference/stdlib.md](reference/stdlib.md) |
| Understand an error code | [reference/errors/](reference/errors/) (generated from snapshots) |
| Read the versioning / release policy | [reference/versioning.md](reference/versioning.md) |
| Know the authoritative rules | [spec/](spec/) — see below |
| See what's planned or in progress | [plans/](plans/) |
| Read exploratory notes & idea banks | [research/](research/) |
| Build / install with Nix | [dev/nix.md](dev/nix.md) |

## spec/ — the authoritative surface

These are binding. When they disagree with anything else, they win.

- [philosophy.md](spec/philosophy.md) — ranked priorities; settles arguments.
- [spec.md](spec/spec.md) — the living language spec (what exists today).
- [syntax-decisions.md](spec/syntax-decisions.md) — the owner's control surface;
  the **only** home for ratified syntax decisions.
- [architecture.md](spec/architecture.md) — pipeline (lex → parse → sema →
  codegen) + rules R1–R7.
- [diagnostics.md](spec/diagnostics.md) — error voice + exact render format;
  snapshot-pinned.
- [roadmap.md](spec/roadmap.md) — milestones and exit criteria.
- [decision-ballots.md](spec/decision-ballots.md) — the owner's *open* decision
  queue only (ratified items live in syntax-decisions.md).

## plans/ — implementation plans

Milestone plans by epoch ([epoch-1/](plans/epoch-1/), [epoch-2/](plans/epoch-2/),
[post-epoch-2/](plans/post-epoch-2/)) and the
[jetpack & jetos](plans/jetpack-jetos/README.md) track (package manager + OS),
which keeps its detailed design-of-record docs alongside the plan.

## research/ — exploratory, non-binding

Cross-language idea banks ([syntax-gallery.md](research/syntax-gallery.md),
[feature-considerations.md](research/feature-considerations.md)) and case
studies. Nothing here is decided until it lands in `spec/`.
