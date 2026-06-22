# Plan: Parser correctness policy for protocols/configs

**Status:** planned. Some dependency-policy changes may need owner approval; the first
step is audit and contract tightening.

## Goal

Identify hand-rolled parsers where correctness risk is too high, then either narrow the
documented contract or justify a dependency exception.

## Targets

- LSP JSON
- Manifest TOML subset
- SemVer/version ranges
- C prototype parser
- Any registry/advisory formats

## Implementation Steps

1. For each parser, write down the accepted grammar and rejected cases.
2. Add adversarial tests around escapes, unicode, nesting, malformed input, and recovery.
3. Fix small correctness holes directly.
4. For large protocol surfaces, prepare an I6 exception proposal or explicitly narrow the
   feature contract in docs.
5. Ensure diagnostics stay Jet-owned even if an external parser is eventually used.

## Verification

- Parser-specific unit tests.
- Fuzz-style corpus tests where cheap.
- Docs update for accepted subset.
