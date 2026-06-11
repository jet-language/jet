# Milestone implementation plans

One file per milestone, M3 through M14. Each plan is written so that an
agent can implement the milestone with no design work of its own. The
plans are the *how*; docs/00–05 remain the *what* and *why* and always
win on conflict.

## Protocol for the implementing agent (read this first, every time)

1. Read, in order: docs/00-philosophy.md, docs/02-syntax-decisions.md,
   docs/03-architecture.md, docs/04-diagnostics.md, then your plan file.
2. **Syntax gate.** Your plan lists the decision IDs it depends on under
   "Blocked on decisions". Check docs/02: every listed ID must be
   **Ratified** (or Provisional and explicitly allowed by the plan).
   If one is still open, STOP and report to the owner — do not invent
   syntax, do not pick an option yourself (invariant I7, CLAUDE.md
   protocol). Plans show example code using the *recommended* option
   from docs/06-decision-ballots.md; if the owner ratified a different
   option, substitute it everywhere mechanically.
3. Work test-first: for each feature, write the failing ui fixture or
   example before the code. Snapshot text must follow docs/04 voice
   rules exactly.
4. Build in pipeline order: syntax.rs → lexer → parser → sema → codegen,
   never skipping sema into codegen (rules R1/R2).
5. Error codes: claim them in docs/04's registry as you go. Each
   milestone has a reserved block (M3=E03xx, M4=E04xx, M5=E05xx,
   M6=E06xx, M7=E07xx, M8=E08xx, M9=E09xx, M10=E10xx, M11=E11xx,
   M12=E12xx). Teaching errors continue the shared E0019+ block.
   Lints take L-prefixed codes in the milestone's block (L0301, …).
6. Definition of done (every milestone): all exit criteria pass as
   tests; `cargo test` fully green; every new diagnostic has a snapshot;
   every new feature has an example with expected output; docs/01-spec.md
   updated to describe the new behavior; docs/04 registry updated;
   docs/05 milestone marked done with date; no invariant bent; zero new
   external crates in the compiler (I6 — tooling-binary exceptions must
   be pre-approved in the plan or by the owner).
7. Commit at the end with message `M<N> verified`. Do not start the
   next milestone in the same run.

## One-line prompts (owner: copy one per milestone, in order)

- M3:  `Implement milestone M3 exactly per docs/plans/m03-data.md, following the protocol in docs/plans/README.md.`
- M4:  `Implement milestone M4 exactly per docs/plans/m04-errors.md, following the protocol in docs/plans/README.md.`
- M5:  `Implement milestone M5 exactly per docs/plans/m05-collections.md, following the protocol in docs/plans/README.md.`
- M6:  `Implement milestone M6 exactly per docs/plans/m06-tooling.md, following the protocol in docs/plans/README.md.`
- M7:  `Implement milestone M7 exactly per docs/plans/m07-ffi.md, following the protocol in docs/plans/README.md.`
- M8:  `Implement milestone M8 exactly per docs/plans/m08-closures.md, following the protocol in docs/plans/README.md.`
- M9:  `Implement milestone M9 exactly per docs/plans/m09-generics-traits.md, following the protocol in docs/plans/README.md.`
- M10: `Implement milestone M10 exactly per docs/plans/m10-stdlib.md, following the protocol in docs/plans/README.md.`
- M11: `Implement milestone M11 exactly per docs/plans/m11-concurrency.md, following the protocol in docs/plans/README.md.`
- M12: `Implement milestone M12 exactly per docs/plans/m12-packages.md, following the protocol in docs/plans/README.md.`
- M13: `Implement milestone M13 exactly per docs/plans/m13-lsp.md, following the protocol in docs/plans/README.md.`
- M14: `Implement milestone M14 exactly per docs/plans/m14-v1.md, following the protocol in docs/plans/README.md.`

## Dependency graph

```
M3 (data) ─► M4 (errors) ─► M5 (collections) ─► M6 (tooling I) ─► M7 (FFI)
                                   │
                                   └─► M8 (closures) ─► M9 (generics/traits)
                                            │                  │
                                            ▼                  ▼
                                       M11 (concurrency)  M10 (stdlib)
                                                                │
                                            M12 (packages) ◄────┘
                                                  │
                                            M13 (LSP v2) ─► M14 (v1.0)
```

Strict order M3→M4→M5→M6→M7 first; after M7, M8→M9→M10→M11→M12→M13→M14.
