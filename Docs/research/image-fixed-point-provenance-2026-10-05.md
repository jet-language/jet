# Compiler image fixed-point provenance (2026-10-05)

Scope: a fixed-point image mismatch traced to backend build identity.

- JET_COMPILER_SOURCE_ID included backend build facts.
- Fixed on fix/image-diff at ad5a60a94 (Compiler/Bootstrap/BuildIdentity.rs:127-132).
- The post-fix proof was not run when the report was written.

Source: ImageDiff/REPORT.md.
