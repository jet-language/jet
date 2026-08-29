# Prompt: build the Jet Tower port (dogfood scale canary, side by side)

Copy-paste target: run this in a fresh thread. Read this whole file before acting. You ARE the implementer/orchestrator for this work — plan, dispatch Luna lanes, integrate, verify, and report per docs/agents/orchestration.md.

## Mission

Port the Tower board to Jet as a complete, working, side-by-side twin of the Node app. This is ratified decision D-MEGAPROJ1 (card #2327, ratified 2026-08-28): one of two megaproject scale canaries whose job is to hit Jet's scale walls before any professional team does. The program is the probe; every language friction, bug, missing stdlib verb, and slow tool you hit is a first-class deliverable, not an obstacle.

**Non-replacement law (owner, verbatim intent): the current Node Tower is NOT replaced.** The two versions run side by side so the owner can test and compare. You never edit `plugins/tower/**`. The Jet port lives entirely in its own tree.

## Hard safety laws (violating any of these is stop-and-fix)

1. **Never write the canonical board.** `plugins/tower/.tower/` (main checkout) is the only board of record. The Jet port reads EXPORTED SNAPSHOT COPIES only — copy `tower.json`/`history.json`/`config.json` into your own fixture dir; never open the live files for write, never point the port's store at the live dir.
2. **Never run a second server that could be mistaken for the real one.** The standing rule: agents never run the Node `tower serve`. The Jet port's own server binds a DIFFERENT port (default 8090), prints a `JET TOWER SHADOW — read-only snapshot` banner, and serves only your snapshot copies. It has no write endpoints in this phase; a mutation request returns a clear refusal.
3. **Board writes for work tracking still go through the Node CLI** (`node plugins/tower/tower.mjs …`) like every other agent.

## Where it lives

- Code: `dogfood/tower/` (new top-level `dogfood/` root houses both scale canaries; the sibling jetpack canary is a separate thread).
- It is a normal Jet package: `package.jet`, `src/`, `tests/`, runnable with `jet run` / `jet dev` / `jet test`.
- Proposed structure (from the ratified ballot; adjust with evidence, log deviations):

```
dogfood/tower/
  package.jet
  src/
    model/      cards, decisions, questions, lanes, the computed-lane law, archive law
    store/      snapshot load (tower.json/history.json/config.json), journal/event replay
    server/     http service, sse stream, json api mirroring the Node app's routes
    board/      web ui: board view, focus mode, papercuts, status
    lint/       board-health checks (the tower lint rules)
  tests/        parity suite vs Node outputs
```

## Parity is the acceptance bar

The Node app is the oracle. For a given snapshot fixture:

- `tower state` JSON from the Node CLI vs the Jet port's state endpoint: semantically equal (define and document the comparison — field order and whitespace may differ; values may not).
- Computed lanes, progress, open counts, and lint findings match the Node app exactly on at least three real snapshots taken on different days.
- The board UI renders the same cards in the same lanes; screenshot-compare is not required, state parity is.
- Keep a `tests/parity/` suite that takes a snapshot dir + recorded Node outputs and diffs them; it runs under `jet test`.

## Scale-canary duties (this is why the card exists)

Measure and record from day one, gauntlet-style, appended to `dogfood/tower/METRICS.md` at every integration point:

- cold `jet build` and warm rebuild after a one-line edit, wall time;
- `jet run` and `jet dev` first-result latency;
- LSP responsiveness on the largest file (subjective note is fine until timing exists; `JET_TIMING=1` gives per-request LSP latency);
- error-cascade quality: once a week of work, seed one deliberate breaking change, record how many diagnostics fire and whether the first one names the real cause;
- LOC and token counts vs the Node implementation for the same functionality.

Every wall you hit gets recorded ON THE SPOT: a Jet compiler/stdlib bug is a Tower card (P0 if a default-tier silent divergence); in-repo tooling friction that meets all four papercut gates is a papercut; a missing stdlib verb or painful pattern is a card tagged `dogfood,scale`. Never hack silently around a language defect — card it, then work around it with a comment naming the card number.

## Working rules

- Read `docs/agents/orchestration.md` and follow it: you orchestrate, Luna max workers implement disjoint lanes (one lane per `src/` package is a natural split), workers type-check only (`scripts/agent/lane-check.sh` + `./target/debug/jet check` on files they write), you integrate and run proof.
- Everything through `scripts/agent/jet-env`. Rebuild `target/debug/jet` before smoke tests. `/tmp` is RAM-backed — scratch to `~/.cache`; respect `JET_TARGET_CAP_GB`.
- Jet code follows current ratified syntax; run `jet fmt --check` on written files. Default tier first: the port must work under `jet run`; exercise `jet dev` for the UI loop. AOT build proof at integration points.
- Log progress on card #2327 (`tower card log`). Mint child implementation cards with `--refs 2327` when a slice needs its own tracking. Do not close #2327 — it is the standing canary card.
- Commit only your owned paths (`dogfood/tower/**`, Tower store via CLI). Never `git add -A`. No `#N` refs in commit messages (githook trap).

## Deliverables

1. Working Jet Tower shadow: loads real snapshots, serves state + board UI on :8090, passes the parity suite under `jet test`.
2. `dogfood/tower/METRICS.md` — the running scale ledger described above.
3. A comparison report at `docs/audits/dogfood-tower-<date>.md` (owner doc rules: no hard-wrapped prose, visual-first, `simple` prose): Jet vs Node side by side — LOC, tokens, RSS, startup, request latency, and the honest friction list with card numbers. This report is what the owner uses to compare the two versions.
4. All dogfood findings carded; final message names tests run, commits, open cards minted, and the strongest current scale bottleneck.
