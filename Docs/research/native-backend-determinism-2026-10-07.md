# Native backend determinism (2026-10-07)

Scope: determinism check of the freestanding native path, comparing serial and parallel builds.

Result: 27 paired stems produced byte-identical output serial vs parallel.

This is not a fixed-point claim: it does not show that the native compiler reproduces itself. See proposals/native-fixed-point-no-rust-bootstrap.md for that question. Source: SeedWork1/DETERMINISM-NOTES.txt.
