# Maturity metadata

`#Meta(maturity: …)` tells readers how stable a public API is without changing
compiler behavior. This page is for API authors and documentation tooling. The
closed values are registered in
[`crates/jet-foundation/src/Syntax/package_files.rs`](../../../crates/jet-foundation/src/Syntax/package_files.rs),
and the source-backed example is
[`examples/features/syntax/maturity_tags.jet`](../../../examples/features/syntax/maturity_tags.jet).

## The three values

Each value marks one declaration:

```jet
#Meta(maturity: .Experimental)
fn experimental_label() -> String { "exp" }

#Meta(maturity: .Tested)
fn tested_label() -> String { "tested" }

#Meta(maturity: .Hardened)
fn hardened_label() -> String { "hard" }
```

Attach the same metadata to public API declarations. Fallible declarations
retain the current `-> Type !Error` ordering; the metadata does not change the
function signature.

These values are closed:

- `.Experimental` marks an API that may still change.
- `.Tested` marks an API with normal test coverage and expected stability.
- `.Hardened` marks an API held to the strongest compatibility and review bar.

## Contract

This is **D-MARK-META1=B**. Maturity metadata does not propagate through callers.
The compiler does not warn, error, or alter code generation based on maturity.
The metadata is parsed and formatter-preserved, but has zero semantic and
code-generation effect.

Use the field in API documentation, examples, package READMEs, and generated
documentation. Do not use it for access control, effect ceilings, dependency
policy, or release gating. The values exist only as `maturity` fields of
`#Meta(...)`; standalone `#Experimental`, `#Tested`, `#Hardened` are not grammar.
