# Explicit move/copy census: Rust, C++, Swift, Mojo, Jet (2026-10-06)

Scope: a census of explicit move and copy operations per 1k calls across five language trees. No builds were run.

Results:

- 605 rows in total.
- Rust pooled M/C/E per 1k calls: 2.41 / 12.02 / 39.00.
- Jet project median per 1k calls: 2.11 / 10.29 / 24.94 (n=4).

Caveats: one project per domain; the Jet tree may have changed during the scan.
