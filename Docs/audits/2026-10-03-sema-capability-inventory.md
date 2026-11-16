# Sema capability inventory (2026-10-03)

Scope: source-level inventory of sema features with missing capability, recorded 2026-10-03. Findings are from source reading only; they were not exercised.

- Findings SF-001 through SF-014, and SF-L01 through SF-L03.
- 171 Rust diagnostic codes have no sema-side producer.

Source: SweepFeatures/findings.md and all-sema-absent-diagnostics.md.

## Current-grammar source repair (2026-10-09)

The `JetCheckLead/r3/Compiler-JetSema/parse/diagnostics.json` census contained
37 errors. Mapping its byte spans through `unit.jet` source-marker comments
and reading the current Jet lexer/parser classified seven as frozen-parser
false positives: four mutable collecting-loop bindings, the nested value-`if`
tail and its associated value-block error in `CallablePolicies.jet`, and the
fixed-list literal `[MIRSerdeCodec#2]{.Encode, .Decode}`.

The remaining 30 reports include recovery cascades from genuine source errors:
reserved `derive`/`module` identifiers, arrow fallbacks after braced statement
conditionals, a table fallback captured by a nested one-line conditional, an
unmatched close after the swizzle checker, and a duplicated registration
constructor block. The source repairs preserve collecting-loop mutability and
the fixed-list literal. Existing nested statement-loop body repairs were also
retained because the current parser requires braces around nested controls.

History evidence: `d67330dbb` introduced the affected derived-trait traversal,
dimension ownership fields and swizzle close; `4d77d5f9d` introduced the
recursive pattern/coverage bodies. This is source/history classification,
not a successful self-compilation claim. No builds, tests, linters or verifier
runs were performed for this repair.
