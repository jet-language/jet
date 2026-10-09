# AES-GCM constant-time design (2026-10-08)

Scope: design notes for a constant-time AES-GCM implementation. The design was not exercised; no test or benchmark results are recorded.

Design points:

- Bitsliced S-box for AES.
- GHASH for the GCM authentication path.

Source: RtCrypto2/AES-CT-DESIGN.md.
