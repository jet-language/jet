# Prompt: port jetpack to Jet (dogfood scale canary, side by side)

**Status — owner pause, 2026-08-30:** do not resume parity, extension, or replacement work on the Jet port. The Rust jetpack is the sole active implementation until it is fully functional, reliable, and stable. Keep the existing Jet canary as captured evidence; do not maintain it in parallel. This pause does not reject eventual replacement. Resume only after an explicit owner instruction.

Historical copy-paste target. Do not run this prompt while the owner pause is active. When reactivated, read the whole file before acting. You ARE the implementer/orchestrator for this work — plan, dispatch Luna lanes, integrate, verify, and report per docs/agents/orchestration.md.

## Mission

Port jetpack — Jet's package/environment manager — to Jet itself, as a side-by-side twin of the shipped Rust implementation. This is ratified decision D-MEGAPROJ1 (card #2327, ratified 2026-08-28): one of two megaproject scale canaries, and an explicit down payment on e11 self-hosting (refs card #217 — compiler-shaped code, arenas, journals, stores). The program is the probe; every language friction, bug, missing stdlib verb, and slow tool you hit is a first-class deliverable, not an obstacle.

**Canary isolation law (historical campaign constraint): the shipped jetpack was not replaced or edited during the dogfood campaign.** The two versions ran side by side so the owner could test and compare. The Jet port lived entirely in its own tree and used a distinct binary or entry. This was an isolation rule for the campaign, not an owner verdict against eventual replacement.

## Hard safety laws (violating any of these is stop-and-fix)

1. **Own store only.** The port NEVER touches the real hangar store, runtime caches, or lock/journal files of the main checkout or the user's home (`~/.cache/jet*`). All state goes under one root you own, defaulting to `~/.cache/jet-dogfood/jetpack-store/`, overridable by a flag/env your port defines. Reads of REAL project inputs (`package.jet`, `.jet/lock`) are fine; every write lands in your own root or a scratch project dir.
2. **No network by default.** Provider fetches (nixpkgs, github) run only in explicitly network-marked test phases, mirroring jetpack's own offline guarantee; parity work uses recorded transcripts first.
3. **Never mutate the shared build tree state**: no cargo invocations from workers (type-check via `lane-check.sh` only), builder-side proof per orchestration doc.

## Where it lives

- Code: `dogfood/jetpack/` (sibling of the Tower canary at `dogfood/tower/`, separate thread).
- A normal Jet package: `package.jet`, `src/`, `tests/`, runnable with `jet run` / `jet test`.
- Scope is phased; each phase has its own parity bar. Do not start a later phase before the earlier one's parity suite is green:

```
dogfood/jetpack/
  package.jet
  src/
    model/      package model: manifest parse, deps, variants, refs (target@provider)
    lock/       .jet/lock read/write (own copies), digest verification
    plan/       resolution + plan rendering (the read-only verbs)
    store/      hangar store v2 path law, ingest, journal, compaction — own root only
    realize/    provider resolution: local paths first, then recorded-transcript providers
    cli/        verb surface mirroring jetpack's grammar (plan, list, inspect, realize)
  tests/        parity suite vs shipped jetpack transcripts
```

- Phase 1 — read-only verbs: parse real `package.jet` manifests and locks, produce plan/list/inspect output. Parity: byte- or semantically-equal (documented) output vs shipped `jetpack` on the same inputs.
- Phase 2 — store: hangar path law (reserved names, case-fold collisions, no implicit normalization), ingest into the OWN store root, journal + compaction behavior on synthetic fixtures. Parity: same accept/reject verdicts and store layout as shipped jetpack driven against a throwaway store.
- Phase 3 — realization of local-path and pre-fetched packages end to end in the own store; recorded transcripts stand in for network providers.

## Parity harness

The shipped `jetpack` binary is the oracle. Build a transcript recorder early: run shipped jetpack on a fixture project (throwaway store), capture argv, stdout, exit code, and resulting store tree; the Jet port replays the same argv and the suite diffs all three. Transcripts live in `dogfood/jetpack/tests/transcripts/`; the suite runs under `jet test`. Divergence buckets: port bug (fix), shipped-jetpack bug (Tower card — dogfooding gold), or ambiguity in the spec (card, cite the spec section).

## Scale-canary duties (why the card exists)

Same ledger discipline as the Tower canary, appended to `dogfood/jetpack/METRICS.md` at every integration point: cold/warm `jet build` wall time, `jet run` first-result latency, LSP feel on the largest file (`JET_TIMING=1`), one seeded breaking change per week of work with error-cascade notes, LOC/tokens vs the Rust implementation for matched functionality.

Every wall recorded on the spot: Jet compiler/stdlib bug → Tower card (P0 for default-tier silent divergence); missing stdlib verb or painful pattern → card tagged `dogfood,scale`; in-repo tooling friction meeting all four papercut gates → papercut. Never hack silently around a language defect — card it, then work around it with a comment naming the card number. This port will stress exactly what self-hosting needs: byte handling, hashing, path law, journals, process spawning — expect stdlib gaps and card them precisely.

## Working rules

- Follow `docs/agents/orchestration.md`: you orchestrate, Luna max workers implement disjoint lanes (one per `src/` package), workers type-check only, you integrate and prove.
- Everything through `scripts/agent/jet-env`. Rebuild `target/debug/jet` before smoke tests. `/tmp` is RAM-backed — scratch to `~/.cache`; respect `JET_TARGET_CAP_GB`.
- Current ratified syntax; `jet fmt --check` on written files; default `jet run` tier is the primary proof tier, AOT build at integration points.
- Log progress on card #2327 (`tower card log`); mint child cards `--refs 2327` per phase. Do not close #2327 — it is the standing canary card. Board writes via the Node CLI only.
- Commit only owned paths (`dogfood/jetpack/**`, Tower store via CLI). Never `git add -A`. No `#N` refs in commit messages (githook trap).

## Deliverables

1. Phase 1 minimum in one thread: read-only verbs at parity with shipped jetpack on real repo manifests, suite green under `jet test`. Phases 2-3 as far as the thread allows, each behind a green parity suite.
2. `dogfood/jetpack/METRICS.md` — the running scale ledger.
3. Comparison report at `docs/audits/dogfood-jetpack-<date>.md` (owner doc rules: no hard-wrapped prose, visual-first, `simple` prose): Jet vs Rust side by side — LOC, tokens, binary size, startup, verb latency, and the honest friction list with card numbers. This is what the owner uses to compare the two versions.
4. All dogfood findings carded; final message names tests run, commits, phase reached, open cards minted, and the strongest current scale bottleneck.
