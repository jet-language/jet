# To-Do List

- Support for labeled loop "blocks"? D-lang supports this. **❓ UNTRACKED — appears only here; no ballot/plan. Would amend S19 loop grammar.**
- Relook @audit/@unsafe - just provide an ("audit text") between @unsafe & {}? 
- JetOS - Nameable generations
- Major structural overhaul for jet lang/binary
- Separate lsp, fmt, lint into separate tools from jet binary
- Consider just using "if" for both "if statements" & "when/switch/match case" expressions.
- Support jet commands without requiring specified file .jet extension if the passed file is a .jet file: i.e. `jet run examples/test` in addition to `jet run examples/test.jet`.
- Ensure we support multiple constructor types **🟡 PARTIAL — U18 inferred constructors + S29 explicit `Type { … }` ratified & implemented (syntax-decisions.md). A "multiple named/secondary constructors per type" feature is not a distinct tracked item (factory form was rejected in S29); clarify if more is meant.**
- Optional-chaining / unwrap ergonomics (?., ??, guard/if let): Swift, Kotlin, C#, Dart. Jet has T?/or; round it out. (§12) **🟡 PARTIAL — `??` shipped (S35/S71) and field `?.` shipped; method `?.` staged (D-SUGAR6), refutable binds D-PAT3.**
- **D-JSON1 follow-up (lenient coercion ratified):** Lenient JSON decode is intentional ("8080" → 8080), but should not be invisible. Brainstorm a way to surface coercions — e.g. a per-build or per-decode report file/output that logs what was silently converted. No errors, no breakage; just make the magic legible. A compiler report output file was one idea.
- **REPL std.io preload (ratified A):** Auto-import std.io in the REPL so `print` etc. just work. On the first use of an auto-imported symbol, print a one-line teaching note showing that it was auto-imported and what the equivalent explicit import is. Benefit of magic without hiding the model.

## Bug Fixes

- Keyword recognition is broken in lsp -> user can't use "keywords" as variable names even when not in keyword positions. **❓ DESIGN DECISION NEEDED — keywords are currently hard-reserved by the lexer; allowing contextual use (e.g. `val fn = ...`) requires a syntax decision (raw-ident `r#fn`? contextual parsing? owner to decide).**
- functions that are not public can still be called - is this intended? **✅ CONFIRMED INTENDED — within the same file, all functions are accessible; `pub` is cross-module only (same as Rust). No bug.**
- Modules are broken - overly implicit ties to jetos "modules" - completely non functional as originally intended. **❓ DESIGN DECISION NEEDED — `module { … }` is parsed as a JetOS unit declaration (sources/imports/contributions). General-purpose code namespacing uses multi-file `use`. Clarify intended behavior and whether `module` should also scope Jet code.**
- Can't directly print an Int. **✅ CANNOT REPRODUCE — `print(42)`, `print(x)` where x is Int, and `print(field)` all work. If you have a specific failing case, please provide it.**
- ✅ **FIXED** — Fan-out operator used as a statement fired E0003 (`print.[e, f, g]` wrongly rejected). Three fixes applied: (1) parser now recognises `FanOut` as an effectful statement; (2) sema returns `None` for void-callee fan-outs instead of wrong param type; (3) codegen routes through `emit_call` for ident callees so builtins like `print` emit correctly.
- this line computes a value but doesn't do anything with it: 
    ```jet
    print.[e, f, g, a, b, c];
    ```
-
