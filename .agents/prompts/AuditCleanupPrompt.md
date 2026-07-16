# Audit and simplify code structure

Audit the requested scope, then implement high-confidence structural cleanup
without changing behavior. Follow `AGENTS.md`; use `ponytail:ponytail` to prefer
deletion, existing modules, standard library facilities, and simple file moves
over new abstractions or dependencies.

Before editing, inspect Git/Tower ownership and relevant tests. Name the exact
paths owned by this change. Do not front-load unrelated specs. Establish a
behavioral baseline with the narrowest useful build, tests, examples, or
snapshots.

Improve navigation and cohesion only where evidence supports it:

- split files that mix distinct responsibilities;
- group related modules under clear existing concepts;
- remove dead indirection, duplication, compatibility layers, and speculative
  extension points;
- preserve public APIs, diagnostics, generated output, and runtime behavior;
- avoid cosmetic churn and repo-wide renames without a concrete payoff.

One implementer owns each coherent refactor. Concurrent writes use a recorded
worktree and follow the integration/removal lifecycle in `AGENTS.md`. Stage only
owned paths; never use `git add -A`.

Run focused proof after each slice. Then require a fresh Sol review, implementer
fixes and recheck, followed by a fresh Terra review, fixes and recheck. End with
changed paths, behavior-preservation evidence, tests, reviews, and worktree
cleanup proof.
