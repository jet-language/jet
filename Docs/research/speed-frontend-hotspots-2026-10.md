# Frontend speed hotspots on bench30k (2026-10)

Scope: profile of the frontend on the bench30k workload, with ranking of phase costs. Measurements are from the source notes and are not re-measured here; re-measure before citing.

Findings:

- Frontend time on bench30k: 29.37 s, 3.55× faster than the Oct-4 candidate cand41f (recorded).
- Optimizer serial steps account for 92% of optimize wall time.
- Instruction profile: about 554G instructions, about 18M per line. Emit 216G (39%; Rust-text passes fold_find 5.7%, compact_function_text 3.3%); optimize 98G (18%; mir_digest 10.5% of phase); lower 64G (11.5%, malloc-led).
- JetInt as bignum and clone-heavy lists dominate allocator and memmove time.
- Durable fix named in the source: native backend (no Rust text emission).

Sources: Main/speed-helper-results.md (2026-10-09); SpeedLead3 and SpeedLead4 HANDOFF.md (ranking). Speed ranking lever list is in compiler-speed-levers-2026-10.md.
