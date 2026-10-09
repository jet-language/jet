# Native fixed point without a Rust bootstrap

Scope: a proposal to reach the compiler's fixed point with a pinned native seed instead of a Rust-built stage 0.

Design (from the source):

- Pinned seeds jetc1, jetc2 and jetc3 must be byte-identical when rebuilt.
- Stage 0 would need about 22 GB; jetc0 would take about 750 s.

Decisions this touches (named in the source, not yet recorded as decided): D-BOOT-SEED1, D-CUTOVER-TARGETS1, D-RELEASE-BACKEND1. Alternatives are to be decided in Tower, not here.

Source: SeedBootstrap1/DESIGN.md. Related evidence: research/native-backend-determinism-2026-10-07.md.
