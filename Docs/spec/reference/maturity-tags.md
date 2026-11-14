# Maturity metadata

`#Meta(maturity: …)` tells readers how stable a public API is without changing
compiler behavior. This page is for API authors and documentation tooling. The
closed values are registered in
[`PackageFiles.jet`](../../../Compiler/JetFoundation/Data/Syntax/PackageFiles.jet),
and the source-backed example is
[`Examples/Metaprogramming/Attributes/MaturityTags.jet`](../../../Examples/Metaprogramming/Attributes/MaturityTags.jet).

## The three values

Each value marks one declaration:

```jet
#Meta(maturity: .Experimental)
experimental_label: fn() -> String { "exp" }

#Meta(maturity: .Tested)
tested_label: fn() -> String { "tested" }

#Meta(maturity: .Hardened)
hardened_label: fn() -> String { "hard" }
```

Attach the same metadata to public API declarations. Fallible declarations
retain the current `-> Type Error!` ordering; the metadata does not change the
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
