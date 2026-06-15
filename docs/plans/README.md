# Milestone implementation plans

One file per milestone, M3 through M14. Each plan is written so that an
agent can implement the milestone with no design work of its own. The
plans are the *how*; docs/spec/philosophy.md–05 remain the *what* and *why* and always
win on conflict.

Special product tracks may live under `docs/plans/<track>/` when they cut
across milestones. The active example is
[`jetpack-jetos`](jetpack-jetos/README.md): Phase 1 is the `jetpack`
package/environment engine exposed directly as `jetpack
run/build/list/clean/add/remove`, and Phase 2 is the `jetos` distro/ISO built
on top of it. Its remaining open D-JPK decisions must be ratified before
implementation.

Post-v1 Epoch 2 plans live in [`epoch-2/`](epoch-2/README.md). Milestone
**E2-M18** (`jet repl`) is draft-only until Group 12 ballots (D-REPL1…21) in
docs/spec/decision-ballots.md are ratified — see
[`epoch-2/m18-repl.md`](epoch-2/m18-repl.md).

Post–Epoch 2 deferred work (e.g. C-header auto-binding) lives in
[`post-epoch-2/`](post-epoch-2/README.md).

## Protocol for the implementing agent (read this first, every time)

1. Read, in order: docs/spec/philosophy.md, docs/spec/syntax-decisions.md,
   docs/spec/architecture.md, docs/spec/diagnostics.md, then your plan file.
2. **Syntax gate.** Your plan lists the decision IDs it depends on under
   "Blocked on decisions". Check docs/spec/syntax-decisions.md: every listed ID must be
   **Ratified** (or Provisional and explicitly allowed by the plan).
   If one is still open, STOP and report to the owner — do not invent
   syntax, do not pick an option yourself (invariant I7, CLAUDE.md
   protocol). Plans show example code using the *recommended* option
   from docs/spec/decision-ballots.md; if the owner ratified a different
   option, substitute it everywhere mechanically.
3. Work test-first: for each feature, write the failing ui fixture or
   example before the code. Snapshot text must follow docs/spec/diagnostics.md voice
   rules exactly.
4. Build in pipeline order: syntax.rs → lexer → parser → sema → codegen,
   never skipping sema into codegen (rules R1/R2).
5. Error codes: claim them in docs/spec/diagnostics.md's registry as you go. Each
   milestone has a reserved block (M3=E03xx, M4=E04xx, M5=E05xx,
   M6=E06xx, M7=E07xx, M8=E08xx, M9=E09xx, M10=E10xx, M11=E11xx,
   M12=E12xx). Teaching errors continue the shared E0019+ block.
   Lints take L-prefixed codes in the milestone's block (L0301, …).
6. Definition of done (every milestone): all exit criteria pass as
   tests; `cargo test` fully green; every new diagnostic has a snapshot;
   every new feature has an example with expected output; docs/spec/spec.md
   updated to describe the new behavior; docs/spec/diagnostics.md registry updated;
   docs/spec/roadmap.md milestone marked done with date; no invariant bent; zero new
   external crates in the compiler (I6 — tooling-binary exceptions must
   be pre-approved in the plan or by the owner).
7. Commit at the end with message `M<N> verified`. Do not start the
   next milestone in the same run.

## One-line prompts (owner: copy one per milestone, in order)

- M3:  `Implement milestone M3 exactly per docs/plans/epoch-1/m03-data.md, following the protocol in docs/plans/README.md.`
- M4:  `Implement milestone M4 exactly per docs/plans/epoch-1/m04-errors.md, following the protocol in docs/plans/README.md.`
- M5:  `Implement milestone M5 exactly per docs/plans/epoch-1/m05-collections.md, following the protocol in docs/plans/README.md.`
- M6:  `Implement milestone M6 exactly per docs/plans/epoch-1/m06-tooling.md, following the protocol in docs/plans/README.md.`
- M7:  `Implement milestone M7 exactly per docs/plans/epoch-1/m07-ffi.md, following the protocol in docs/plans/README.md.`
- M8:  `Implement milestone M8 exactly per docs/plans/epoch-1/m08-closures.md, following the protocol in docs/plans/README.md.`
- M9:  `Implement milestone M9 exactly per docs/plans/epoch-1/m09-generics-traits.md, following the protocol in docs/plans/README.md.`
- M10: `Implement milestone M10 exactly per docs/plans/epoch-1/m10-stdlib.md, following the protocol in docs/plans/README.md.`
- M11: `Implement milestone M11 exactly per docs/plans/epoch-2/m1-concurrency.md, following the protocol in docs/plans/README.md.`
- M12: `Implement milestone M12 exactly per docs/plans/epoch-1/m12-packages.md, following the protocol in docs/plans/README.md.`
- M13: `Implement milestone M13 exactly per docs/plans/epoch-1/m13-lsp.md, following the protocol in docs/plans/README.md.`
- M14: `Implement milestone M14 exactly per docs/plans/epoch-1/m14-v1.md, following the protocol in docs/plans/README.md.`

### Epoch 2 (post-v1; one detailed plan per milestone, blocked on its ballots)

Each plan lists the ballot IDs it is blocked on (docs/spec/decision-ballots.md);
ratify those before starting. E2-M1 is verified.

- E2-M2:  `Implement milestone E2-M2 exactly per docs/plans/epoch-2/m2-release-policy.md, following the protocol in docs/plans/README.md.`
- E2-M3:  `Implement milestone E2-M3 exactly per docs/plans/epoch-2/m3-dx-cli.md, following the protocol in docs/plans/README.md.`
- E2-M4:  `Implement milestone E2-M4 exactly per docs/plans/epoch-2/m4-jet-dev.md, following the protocol in docs/plans/README.md.`
- E2-M5:  `Implement milestone E2-M5 exactly per docs/plans/epoch-2/m5-references.md, following the protocol in docs/plans/README.md.`
- E2-M6:  `Implement milestone E2-M6 exactly per docs/plans/epoch-2/m6-library-authoring.md, following the protocol in docs/plans/README.md.`
- E2-M7:  `Implement milestone E2-M7 exactly per docs/plans/epoch-2/m7-streaming-io.md, following the protocol in docs/plans/README.md.`
- E2-M8:  `Implement milestone E2-M8 exactly per docs/plans/epoch-2/m8-packages-supply-chain.md, following the protocol in docs/plans/README.md.`
- E2-M9:  `Implement milestone E2-M9 exactly per docs/plans/epoch-2/m9-first-party-libraries.md, following the protocol in docs/plans/README.md.`
- E2-M10: `Implement milestone E2-M10 exactly per docs/plans/epoch-2/m10-network-services.md, following the protocol in docs/plans/README.md.`
- E2-M11: `Implement milestone E2-M11 exactly per docs/plans/epoch-2/m11-testing-docs-bench.md, following the protocol in docs/plans/README.md.`
- E2-M12: `Implement milestone E2-M12 exactly per docs/plans/epoch-2/m12-debug-observe.md, following the protocol in docs/plans/README.md.`
- E2-M13: `Implement milestone E2-M13 exactly per docs/plans/epoch-2/m13-low-level-tier.md, following the protocol in docs/plans/README.md.`
- E2-M14: `Implement milestone E2-M14 exactly per docs/plans/epoch-2/m14-c-ffi.md, following the protocol in docs/plans/README.md.`
- E2-M15: `Implement milestone E2-M15 exactly per docs/plans/epoch-2/m15-freestanding-cross.md, following the protocol in docs/plans/README.md.`
- E2-M16: `Implement milestone E2-M16 exactly per docs/plans/epoch-2/m16-pure-eval-layer3.md, following the protocol in docs/plans/README.md.`
- E2-M17: `Implement milestone E2-M17 exactly per docs/plans/epoch-2/m17-epoch2-ga.md, following the protocol in docs/plans/README.md.`
- E2-M18: `Implement milestone E2-M18 exactly per docs/plans/epoch-2/m18-repl.md, following the protocol in docs/plans/README.md.`

## Dependency graph

```
M3 (data) ─► M4 (errors) ─► M5 (collections) ─► M6 (tooling I) ─► M7 (FFI)
                                   │
                                   └─► M8 (closures) ─► M9 (generics/traits)
                                            │                  │
                                            │                  ▼
                                            │             M10 (stdlib)
                                            │                  │
                                            └────────► M12 (packages)
                                                              │
                                            M13 (LSP v2) ◄────┘
                                                  │
                                            M14 (v1.0)

M11 (concurrency) deferred to v2 (S53) — not on the v1 path.
```

Strict order M3→M4→M5→M6→M7 first; after M7, M8→M9→M9.5→M10→M12→M13→M14.

## Example numbering (reserved)

Sequential `examples/NN_*.jet` slots — do not reuse or skip when adding a
milestone example. Multi-file demos use a directory (`examples/features/21_imports/`).

| # | Milestone | File(s) |
|---|-----------|---------|
| 01–09 | M0–M2 | hello, functions, values, branches, fizzbuzz, compound, switch, ownership, ref_field |
| 10–13 | M3–M4 | structs, enums, option, errors |
| 14 | M4 | panic |
| 15–18 | M5 | lists, wordcount, strings, list_bounds |
| 19 | M5 | map_key (error demo) |
| 20 | M6 | tests |
| 21 | M6 | imports/ |
| 22 | M7 | ffi |
| 23–24 | M8 | closures, callbacks |
| 25–26 | M9 | traits, generic_types |
| 27–28 | M9.5 | comptime_table, embed |
| 29–31 | M10 | files, json, cli |
| 32–33 | E2-M1 | tasks, pipeline (per m1-concurrency.md) |
| 34 | E2-M3 | digits |
| 35 | E2-M5 | zerocopy |
| 36 | E2-M6 | library |
| 37 | E2-M7 | stream |
| 38 | E2-M8 | packages/ (supply chain) |
| 39–43 | E2-M9 | csv, toml, log, archive, hash |
| 44–45 | E2-M10 | http_client, http_server |
| 46 | E2-M11 | doctest |
| 47 | E2-M12 | debug |
| 48 | E2-M13 | lowlevel |
| 49 | E2-M14 | cffi |
| 50–51 | E2-M15 | cross, freestanding |
| 52 | E2-M16 | pure |

(E2-M18 REPL has no numbered example — its spec is a `tests/repl/` transcript.)
