# Epoch 2 agent handoff

All owner decisions are ratified. Every milestone from M6 onward is unblocked. This file is your prompt to an agent cluster.

---

## What's done

E2-M1 ✅ M2 ✅ M3 ✅ M4 ✅ M5 ✅ M13 ✅ M14 ✅ (on branch `epoch-2-impl`, merged into `master` via `jetos-ratified-arc`)

---

## What to build next (dependency order)

```
syntax-register-batch   ← do first; unblocks decisions.rs enforcement
s19-amend               ← do first; fixes loop keyword in parser + snapshots

M6 library authoring    ← unblocked now
M7 streaming I/O        ← unblocked now (depends on M6 Fallible trait)
M8 packages             ← after M6
M9 first-party ring     ← after M6 + M8; includes jet.yaml + D-DEP1 pattern
M10 networking          ← after M7 + M9; jet.tls ships as a package (not compiler dep)
M11 testing/docs/bench  ← after M3✅ + M4✅; `todo` hole ships now, D-TOOL5=C
M12 debug/observe       ← after M10; foundation only (no DAP); DAP deferred to M17
M15 cross/freestanding  ← after M13✅ + M14✅; D-CROSS2=abort, D-CROSS3=QEMU doc
M16 pure eval           ← after M8 + M4✅; signed cache ships (D-PURE3=B)
M18 REPL                ← after M4✅; std-only (no rustyline)
M17 GA                  ← last; all 6 showcases mandatory; hard CI perf/size gates; DAP lands here
```

---

## Where to look for each task

Every unimplemented milestone has a sidequest file in `docs/plans/sidequests/` capturing the **key amendments** from recent ratifications:

| File | What changed vs original plan |
|---|---|
| `m6-library-authoring-impl.md` | D-LIB2 ratified (associated types); D-ERR1/D-FP1 still open (skip them) |
| `m7-streaming-io-impl.md` | No amendments; implement directly from plan |
| `m8-packages-supply-chain-impl.md` | D-PKGS4 amended: must compile+CI before registry accepts publish |
| `m9-first-party-ring-impl.md` | D-LR4=B: jet.yaml added to wave 1; D-DEP1 Rust-wrapping pattern |
| `m10-networking-impl.md` | D-NET1: TLS ships as `jet.tls` package (not compiler dep) |
| `m11-testing-docs-bench-impl.md` | D-TOOL2=A: `todo` ships now; D-TOOL5=C: human summary by default |
| `m12-debug-observe-impl.md` | D-OBS1 split: M12 = foundation only; full DAP → M17 |
| `m15-cross-freestanding-impl.md` | D-CROSS2=abort; D-CROSS3=QEMU harness documented |
| `m16-pure-eval-impl.md` | D-PURE3=B: ship signed cache now (not design-only) |
| `m17-ga-impl.md` | D-GA1=B: all 6 showcases mandatory; D-GA2=B: hard CI gates; DAP lands here |
| `m18-repl-impl.md` | D-REPL11 revised to A: std-only (no rustyline) |
| `jetos-d-os4-os6.md` | D-OS4 priority map; D-OS6 user.me alias |
| `json1-coercion-visibility.md` | Owner-todo: surface lenient JSON coercions |
| `dep1-third-party-package-pattern.md` | D-DEP1: how to wrap Rust crates as Jet packages |
| `syntax-register-batch.md` | All recently-ratified S/U/D codes need src/syntax.rs + decisions.rs |
| `s19-amend-loop-unification.md` | Loop keyword unification in parser + snapshots |

For the full milestone spec, read `docs/plans/epoch-2/mN-*.md`. The sidequest files capture only what changed.

---

## Agent prompt (copy-paste to start an agent)

> You are implementing Epoch 2 of the Jet language compiler. All owner decisions are ratified. Read CLAUDE.md first. Then read `docs/plans/sidequests/` — each file describes a discrete implementation task or milestone amendment. The dependency order is in `docs/plans/EPOCH2-HANDOFF.md`. Start with `syntax-register-batch.md` and `s19-amend-loop-unification.md` (they unblock everything else), then proceed in the order listed. For each milestone, read the sidequest amendment file AND the full plan at `docs/plans/epoch-2/mN-*.md`. Follow the workflow loop in CLAUDE.md: failing test first → spec → parser → sema → codegen → `nix develop -c cargo test` green → docs updated → commit. Never touch the jetpack/jetos track files (src/jetpack/*, src/syntax.rs config/U-series) without checking EPOCH2-IMPL-PROGRESS.md for collision avoidance.
