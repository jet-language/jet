# Rust emit scaling (2026-10-01 series, reports 2026-10-03)

Scope: how emitted Rust grows per phase (functions, types, closures, enum/match, generics) as the input doubles.

Findings:

- enum_variants grows 5.03× per doubling step (superlinear).
- match_arms grows 3.27× per doubling step (superlinear).

No later rerun was present in the shard. Sources: EmitSize2/scale1/report.md and scale2/report.md. Covered in Tower by #4601 (planning), #4319 and #4147 (done), according to the mining row.
