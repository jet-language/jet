# Maturity metadata

`#Meta(maturity: …)` tells readers how stable a public API is without changing
compiler behavior.

```jet
#Meta(maturity: .Experimental)
pub fn parse_streaming(src: Stream) -> Doc ? ParseError { ... }

#Meta(maturity: .Tested)
pub fn parse(src: String) -> Doc ? ParseError { ... }

#Meta(maturity: .Hardened)
pub fn parse_strict(src: String) -> Doc ? ParseError { ... }
```

Current contract (D-MARK-META1=B):

- `.Experimental` marks an API that may still change.
- `.Tested` marks an API with normal test coverage and expected stability.
- `.Hardened` marks an API held to the strongest compatibility and review bar.
- Maturity metadata does not propagate through callers.
- The compiler does not warn, error, or alter codegen based on maturity.

Use the field in API docs, examples, package READMEs, and generated
documentation. Do not rely on it for access control, effect ceilings,
dependency policy, or release gating. Standalone `@Experimental`/`@Tested`/
`@Hardened` and `#Experimental`/`#Tested`/`#Hardened` are not grammar.

See also: [Core library](core-library.md),
`examples/features/syntax/maturity_tags.jet`.
