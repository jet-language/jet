# Primitive probe: tooling hooks

## What I built
I built a Jet build hook that calls `core.compiler.lex`, `parse`, and `check`, then emits a generated module containing the observed token, item, and function counts. I built a graph fixture with one declared action, a replay fixture, a lossless rename proof, and a four-request loopback HTTP threshold runner. Files: `pkg/run.jet`, `pkg/input.jet`, `pkg/renamed.jet`, `graph/graph.jet`, `replay/run.jet`, `load/run.jet`, and `rename.mjs`.

## What worked
- `jet build .../pkg`: succeeded; generated `.jet/generated/prim_tooling_hooks/facts.jet` with `tokens={19}; items={2}; functions={2}`.
- `jet inspect compiler lex pkg/input.jet | jq ...`: `status=ok`, schema 1, 39 tokens; two `old_name` identifier spans `{start:42,end:50}` and `{start:181,end:189}`.
- `jet inspect compiler parse pkg/input.jet | jq ...`: `status=ok`; two function items (`old_name`, `run`); diagnostics empty.
- `jet inspect compiler check pkg/input.jet | jq ...`: `status=ok`; two functions; diagnostics empty.
- `node rename.mjs`: `renamed identifiers=2; preserved comment/literal=true`; the renamed file parsed with `status=ok` and `new_name` as the function item.
- `jet inspect graph graph/graph.jet`: `target graph_probe Executable`; `action stamp stamp.txt`. `jet inspect explain-build stamp ...`: argv, one input, one output, `cache=Cached`.
- `jet run run.jet --record=probe` from `replay/`: finalized exit 0 and wrote `.jet/replays/probe.jetproof-replay`. `jet debug run.jet --replay=probe`: breakpoint, `replay ok`, `program finished`. Altered source correctly failed E3621 identity mismatch.
- `jet run --allow-net run.jet --watch=off` from `load/`: `threshold=4/4`.

## Gaps
See `gaps.json`: runtime compiler facts are unavailable (E0956); graph/receipt diffs are not a stable in-process API; generated-module builds are not idempotent (E3510 on the second build).

## Friction and defects
`jet inspect graph pkg/run.jet` fails E0956 because the inspect evaluator cannot execute the same `core.compiler` calls that full `jet build` accepts. Package `authority.holds` did not remove the need for `--allow-net` for the load run. Without that flag the run timed out in HTTP transport. The load runner therefore proves the path only with an explicit CLI grant.

## Research mined
`docs/spec/architecture.md` (compiler seam and build graph), `docs/spec/observability.md` (record/replay), and reports in `~/.cache/jet-luna/dx2/compilers-language-tooling/`, `build-systems-monorepo/`, and `testing-qa-automation/`. The deleted syntax-tree ballot (`D-M-SYNTAX-TREE1`) confirms that current parse facts are top-level items/spans, not a lossless incremental edit tree; the deleted receipts ballot (`D-M-RECEIPTS1`) confirms replay is receipt-backed.

## Verdict
Jet already exposes useful versioned compiler facts, build graph text, replay capture/debug, and deterministic sockets. A small external tool can safely rename identifiers by splicing returned spans while preserving comments and literals. A Jet-native tool cannot inspect arbitrary source at runtime, cannot query receipt deltas without private files, and hits generated-output shadowing on repeat builds. HTTP load code is expressible, but authority requires an explicit `--allow-net` grant.
