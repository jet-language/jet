# Atomic word provider design (2026-10-07)

Scope: design for the native atomic-word provider.

- 24 C exports.
- Ordering fixed at SeqCst.
- Store lowered to XCHG.
- Read-modify-write operations: lock CMPXCHG and XADD.

Source: RtL5c5/ATOMIC-DESIGN.md. The ballot on atomic width is in audits/ballot-review-atomic-width-2026-10.md.
