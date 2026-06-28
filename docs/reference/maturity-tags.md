# Maturity tags

`#Experimental`, `#Tested`, and `#Hardened` are documentation tags for public APIs.
They tell readers how stable an API is without changing compiler behavior.

```jet
#Experimental
pub fn parse_streaming(src: Stream) -> Doc ? ParseError { ... }

#Tested
pub fn parse(src: String) -> Doc ? ParseError { ... }

#Hardened
pub fn parse_strict(src: String) -> Doc ? ParseError { ... }
```

Current contract (D-MATURITY1):

- `#Experimental` marks an API that may still change.
- `#Tested` marks an API with normal test coverage and expected stability.
- `#Hardened` marks an API held to the strongest compatibility and review bar.
- These tags do not propagate through callers.
- The compiler does not warn, error, or alter codegen based on these tags.

Use them in API docs, examples, package READMEs, and generated documentation. Do
not rely on them for access control, effect ceilings, dependency policy, or
release gating.
