# To-Do List

## Next Tasks

All seven sidequest items are planned in `docs/plans/sidequests/`. The five that
needed owner decisions — plus the **D-IF2** follow-up that settled D-IF1's
multi-arm `if` surface — were **ratified 2026-06-18** (recorded in
`syntax-decisions.md`). The decision-ballot queue is now empty.

| Item | Plan | Status |
|------|------|--------|
| Implicit lib/exec if `fn main()` present | `d-ile1-implicit-lib-exec.md` | **Ratified (D-ILE1=A)** — ready to implement |
| Odin-style `:=` / `::` binding sigils | `d-bind1-binding-sigils.md` | **Ratified (D-BIND1=A)** — ready to implement |
| Labeled loops + `break @label` (amends S19/S23) | `d-label1-loop-labels.md` | **Ratified (D-LABEL1=B, `@name`)** — ready to implement |
| No visible `;` end-line operator | `s6-r-no-semicolons.md` | **Ratified (S6-R=B)** — ready to implement |
| `if` as unified conditional (retire `when`) | `d-if1-if-universal.md` | **Ratified (D-IF1=A + D-IF2 surface)** — ready to implement |
| Optional `.jet` extension in CLI | `ext-optional-cli.md` | Ready to implement |
| S19 loop unification (ratified) | `s19-amend-loop-unification.md` | Ready to implement |

## Considerations
- Remove space between type name & constructor block. -> should this just be an inherent dot operator method/function constructor call? 
- Relook @audit/@unsafe - just provide an ("audit text") between @unsafe & {}? 
- JetOS - Nameable generations
- Major structural overhaul for jet lang/binary
- Look into allocators/arena allocators
- Separate lsp, fmt, lint into separate tools from jet binary
- Ensure we support multiple constructor types **🟡 PARTIAL — U18 inferred constructors + S29 explicit `Type { … }` ratified & implemented (syntax-decisions.md). A "multiple named/secondary constructors per type" feature is not a distinct tracked item (factory form was rejected in S29); clarify if more is meant.**
- Optional-chaining / unwrap ergonomics (?., ??, guard/if let): Swift, Kotlin, C#, Dart. Jet has T?/or; round it out. (§12) **🟡 PARTIAL — `??` shipped (S35/S71) and field `?.` shipped; method `?.` staged (D-SUGAR6), refutable binds D-PAT3.**
- **D-JSON1 follow-up (lenient coercion ratified):** Lenient JSON decode is intentional ("8080" → 8080), but should not be invisible. Brainstorm a way to surface coercions — e.g. a per-build or per-decode report file/output that logs what was silently converted. No errors, no breakage; just make the magic legible. A compiler report output file was one idea.
- **REPL std.io preload (ratified A):** Auto-import std.io in the REPL so `print` etc. just work. On the first use of an auto-imported symbol, print a one-line teaching note showing that it was auto-imported and what the equivalent explicit import is. Benefit of magic without hiding the model. - NO I want print to be in stdlib not stdlib io if possible

## Odin Ideas

- The when statement is almost identical to the if statement but with some differences:
  - Each condition must be a constant expression as a when statement is evaluated at compile time.
  - The statements within a branch do not create a new scope
  - The compiler checks the semantics and code only for statements that belong to the first condition that is true
  - An initial statement is not allowed in a when statement when statements are allowed at file scope
  - The when statement is very useful for writing platform specific code
- Supports a switch statement that can use ranges like a range based loop
```odin
switch c := 'j'; c {
		case 'A'..='Z', 'a'..='z', '0'..='9':
			fmt.println("c is alphanumeric")
		}

		switch x {
		case 0..<10:
			fmt.println("units")
		case 10..<13:
			fmt.println("pre-teens")
		case 13..<20:
			fmt.println("teens")
		case 20..<30:
			fmt.println("twenties")
		}
```


## Capstone Findings (2026-06-18)

- `io.args()` strips `--flag` arguments — jet run consumes `--xxx` before the program sees them; CLI programs must use bare words (`json`, `config`) instead of POSIX `--flags`. Decide whether to pass through `--` args or add a `--` separator convention.
- L0201 implicit-clone warnings are too noisy — every `String` passed to a stdlib function that stores it emits a warning; 4 in logbook.jet alone. Consider a smarter ownership model or suppressing the warning for common patterns. Consider implicit optimization that uses str ref instead of cloning if that functions better. 
- `@test fn name { }` (S82) is ratified but not implemented — parser still only accepts `test "name" { }`; the spec and the parser are out of sync.
- `extern rust "std"` `u32` → `Int` boundary mapping is untested — `std::process::id()` returns `u32`; clarify whether FFI boundary type coercions for unsigned int are supported or need a cast helper.

## Bug Fixes

- Keyword recognition is broken in lsp -> user can't use "keywords" as variable names even when not in keyword positions. **❓ DESIGN DECISION NEEDED — keywords are currently hard-reserved by the lexer; allowing contextual use (e.g. `val fn = ...`) requires a syntax decision (raw-ident `r#fn`? contextual parsing? owner to decide).**
- functions that are not public can still be called - is this intended? **✅ CONFIRMED INTENDED — within the same file, all functions are accessible; `pub` is cross-module only (same as Rust). No bug.**
- Modules are broken - overly implicit ties to jetos "modules" - completely non functional as originally intended. **❓ DESIGN DECISION NEEDED — `module { … }` is parsed as a JetOS unit declaration (sources/imports/contributions). General-purpose code namespacing uses multi-file `use`. Clarify intended behavior and whether `module` should also scope Jet code.**
- ✅ **FIXED** — Fan-out operator used as a statement fired E0003 (`print.[e, f, g]` wrongly rejected). Three fixes applied: (1) parser now recognises `FanOut` as an effectful statement; (2) sema returns `None` for void-callee fan-outs instead of wrong param type; (3) codegen routes through `emit_call` for ident callees so builtins like `print` emit correctly.
- this line computes a value but doesn't do anything with it: 
    ```jet
    print.[e, f, g, a, b, c];
    ```
