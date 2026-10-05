---
title: Owner scratch
---

"Error messages are the user interface of your compiler." Jonathan Blow https://youtu.be/e6crOMC9WCE?si=AFgqGl_HUBQ_0cRO

>
>
>
You are resuming the Jetpack-in-Jet port as lead orchestrator (Tower umbrella #3590, milestone e5-m11-jetpack-in-jet) in
/home/nate/Projects/Github/jet. Read AGENTS.md first. Other streams have worked since 2026-09-30, so probe everything before
acting: git log, Tower cards, the current compiler binary.

GOAL (owner order): port Jetpack (Rust; JetOS excluded) to idiomatic Jet under Jetpack/.
1. Build-out: done (~174k lines, 587 files).
2. Stabilization: in progress. Type-check, run on every tier, fix, then a parity harness against Rust Jetpack.
3. Optimization: Jet/Rust ≤1.05 on warm env, store verification, hashing and graph-load cells.
4. Then ask the owner for a cutover decision.

CONSTRAINTS:
- Style: ponytail + caveman, terse replies, also for subagents.
- At most 3 subagents, all Opus (agent: opus-worker); never fall back to another model.
- No OOM risk. Every jet/cargo run goes under `systemd-run --user --scope -p MemoryMax=… -p MemorySwapMax=0 -p
RuntimeMaxSec=…`, plus `timeout`.
- Runs above 6G are allowed with monitoring and safe recovery. Check for orphaned scopes after any tool timeout.
- Scratch goes in ~/.cache/jet-luna, never /tmp.
- Commit only Jetpack/ and your own fix files, using pathspec or index-only commits. Never stage other streams' uncommitted
work.
- If master won't build because of others' edits, build in .agent-worktrees/icefix with the shared target/.
- Owner approved targeted Rust-compiler crash/perf fixes. Close each fix's card with tier evidence (check, jet run,
--interpret, AOT, plus a golden example).
- Update spec/reference docs when a contract changes. Never edit Docs/audits, Docs/research or site/dist.
- No workarounds for compiler defects: file a card with a minimal repro instead.

STATE AS OF 2026-09-30 (verify each item first):
- Read ~/.cache/jet-luna/jetpack-port/pause-resume.md (latest section) and buildout-mode.md. Per-worker reports are in
~/.cache/jet-luna/reports/. Shared names are in ~/.cache/jet-luna/jetpack-port/NAMES.md.
- Harness: `JETPACK_WORKER=<name> Tools/agent/jet-env bash Jetpack/Bootstrap/check.sh [--syntax] [--area X]`.
  - --syntax uses `jet inspect compiler parse`; the whole unit was SYNTAX OK, apart from the #3744 lines in
known-syntax-defects.list.
  - Full checks: env JETPACK_CHECK_MEM / JETPACK_CHECK_SECS. Area graph is Jetpack/Bootstrap/areas.list.
- TOP PRIORITY: #3871 is reopened.
  - Commit dee9a5fe4 sped up small programs, but `jet run` of the 108k-line unit at
~/.cache/jet-luna/IceFix/3676/unit/run/project hits 24G in 18 s. The pre-fix binary held ~3.9G.
  - Compare ~/.cache/jet-luna/IceFix/jet-base against jet-perf (likely the dense drop-flag/dominator storage in
crates/jet-foundation/src/MIROptimization.rs and MIR.rs).
  - The owner was doing "devloop speed-up" work meanwhile. Check whether it superseded or fixed this.
- Then:
  - #3676 verdict.
  - A Foundation area type check: it never finished. Check speed is #3661.
  - #3951: AOT rustc time on large derived enums.
  - #3952: JIT, 30-variant enum ~21 s.
  - #3863, #3864, #3865.
  - Then type-check and run each Jetpack area bottom-up: Foundation, TrustRoot, NixEval, PackageModel, EnvModel, Nix, Store,
Trust, Recipe, Provider, Environment, Services/Image, CLI.
- Owner-gated items:
  - Core ballots ready: D-CORE-PROCFILE1, TERMRAW1, HANDLEFD1, DELREBOOT1. Older open ballots: MODELDESC1, SECRETSTORE1,
NETPUBLIC1, FILEERROR1, ZSTDSOURCE1.
  - #3674 is frozen (already fixed at HEAD); only the owner can close it.
  - Many ratified Core APIs Jetpack calls are not yet implemented (FILEDIR1, FILESTAT1, FILEOPEN1, FILEXATTR1, …). Compiler
entry points are #3599.
- Known parity items: recipe identity byte parity (#3637), and Core vs Rust JSON escaping of DEL/C1 controls and \b/\f.

FIRST STEPS:
1. Run git log since dee9a5fe4.
2. `node Tools/tower/tower.mjs card show '#3871'`, then the same for #3661 and #3590.
3. Rebuild jet under a cap, or confirm target/debug/jet is current.
4. Rerun the 108k-unit memory probe.
5. Report status to the owner briefly before dispatching workers.
>
>
>

Unison
> Should absolutely research for effects, distributed programming/code, hash addressable items/types/functions, incremental compilation, etc.

ReScript
> Pattern matching on dictionaries

Jai does NOT do incremental rebuilds because there can be associated bugs -> yet STILL complete compile time for 300k LOC is under 2 seconds. That is our goal

Lua does MECHANISMS over policies -> give you the features you need at a base level but not 5 keywords to learn
Neovim as inspiration for hooks -> application for data structures/types, accessing compiler internals for metaprogramming, etc
Lua for simplicity?
