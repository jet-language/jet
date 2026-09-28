---
title: Mission audit: philosophy + invariant alignment scores
---
# Mission audit — 2026-07-23

Method: I scored nine mission dimensions against docs/spec/philosophy.md and the AGENTS.md invariants. The evidence comes from four read-only code sweeps (safety/rustc, diagnostics, batteries/examples, package-graph/tools) and from today's surface-audit and lessons-learned notes. Each dimension gets a grade: aligned, drift, or unknown. Each drift gets the smallest corrective action.

## Scores

1. **Safety and hidden rustc (I1/I2/I3) — aligned.** Codegen makes Rust `unsafe` in one place only (`crates/jet-codegen/src/Codegen/TIR/subset/statements.rs:306`). Sema owns the ownership checks (`crates/jet-sema/src/Sema/CheckerOwnership.rs`, 2219 lines, move tracking). When rustc rejects generated code, jet shows an ICE banner and exits 101 (`Source/CmdCompile.rs:2707-2716`, `crates/jet-foundation/src/ExitCodes.rs:36`). Only a missing C library or linker gives a user error instead (E3209/L2101). No "try rustc and see" path exists.

2. **Diagnostics as product (I4) — aligned in structure, one live break.** The `Diagnostic` struct makes what/why/fix mandatory (`crates/jet-foundation/src/Diagnostics.rs:132-146`). About 537 active codes have about 847 snapshots. The test `tests/diagnostics_coverage.rs` checks the registry, the emissions, and the snapshots on every run. `jet explain` reads the spec file directly, so it cannot drift. The break: fix text teaches `jet inspect dossier` and `jet inspect live`, but these commands fail today (surface-audit finding 1). Action: #734.

3. **Beginner defaults — drift in the examples.** The core is correct: no lifetime syntax, single-file `jet run`, and the ratified memory model v5. But I5 makes the examples the executable spec, and the newest examples teach ceremony. They spell static constants as `Duration.seconds(5) ?? panic(...)`, mix two binding forms, and show four ctor shapes in one file. Action: #736/#737 (made today).

4. **Expert control — aligned.** `@Unsafe("reason")` shipped with decision IDs (`crates/jet-foundation/src/Syntax/core_surface.rs:279-288`). core.mem has real allocators (`crates/jet-codegen/src/Prelude/Mem.rs`, 620 lines, D-ALLOC2/D-REGION1). `jet inspect unsafe` gives a deterministic audit report (`Source/CmdUnsafe.rs`, D-UNSAFE-OBLIG1=A).

5. **One mechanism (I8) — aligned in law, drift at the fresh edge.** Syntax.rs has no async keywords. One lazy-protocol slot is reserved. I7 blocks dialects. But commit 27165ba2 shipped a module-factory ctor shape that matches no ratified form. Also, `constant_time_eq` and `constant_time_equal` both live, which breaks D-API-LEN1. Actions: #736 (D-SHAPE-CTORVERB1=C ratified today), #738.

6. **Batteries — aligned.** 35 core modules cover net/http, crypto, encoding/serde, fs/os/path, db, ui/game, text/unicode, math/linalg, reflect, and process. The golden harness finds and runs all 398 examples with no exclusion list (`tests/golden.rs:73-100`). It skips a test only when a toolchain is absent. All 6 sampled recent feat commits have a matching example.

7. **Systems path — aligned, with one stale law note (new).** Vetted prelude code ships the volatile, entropy, and allocator internals, and codegen strips them when unused (`crates/jet-codegen/src/Codegen/mod.rs:456-458`). But `docs/spec/philosophy.md:165-169` still says the I1 amendment is "owner-gated and not yet drafted". AGENTS.md I1 already has the "vetted std/mem internals" carve-out, and shipped code uses it. Smallest action: make philosophy.md match the current I1 text. Cite the decision that ratified the carve-out, or ballot it if none exists. No card covers this.

8. **One package graph (I6) — aligned.** pkg.jet is the only parsed manifest. pack.jet, payload.jet, and jet.toml give teaching error E1226 and have no parse path (`crates/jet-driver/src/Loader.rs:579-595`). All 7 compiler seam crates use path dependencies only, and a test enforces this (`tests/truthfulness.rs:402-552`). The ureq dependency in jet-net is the ratified D-DEP1 exemption, not a seam leak.

9. **Lean tools — drift.** No tool repeats another tool's job. Engine verbs exec the jetpack binary (D-JPK-DISPATCH1=B). `jet install` is a teaching stub (E0043). jetos is a thin front door. But `jet` shows about 80 flat top-level arms, and the ratified D-SHAPE6=A noun groups are not built. This hurts beginner discoverability, and the fix texts already assume the grouped world. Action: #734.

## Mission-level gaps (pointer, not a duplicate)

Today's lessons-learned note names the two untracked mission risks: no language stability/migration law, and no telemetry/trust policy. Neither has a card. The stability law is the larger risk once external users exist.

## Net

The core mission machinery is aligned, and tests enforce most of it. The truthfulness, diagnostics-coverage, and golden tests stand as invariant guards. That is the strongest alignment signal available. Drift sits where fresh surface ships ahead of ratified shape law, and cards #734-#738 track all of it. This audit found one new item: the stale I1-amendment note in philosophy.md (score 7).
