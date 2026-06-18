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


## Memory Capability Model

# Jet Capability Inference System

You are implementing Jet’s ownership and memory-safety system.

The goal is to provide Rust-level safety while exposing a dramatically simpler mental model to users.

Users should think in terms of:

```text
view
edit
take
share
```

Users should not need to think about:

```text
borrowing
lifetimes
ownership graphs
&T
&mut T
```

Those are implementation details of the compiler.

---

# Core Philosophy

Jet tracks capabilities rather than ownership.

Every access falls into one of four capabilities:

```text
view
edit
take
share
```

Definitions:

```text
view  = read-only access
edit  = exclusive temporary mutable access
take  = ownership is consumed and retained
share = multiple owners exist
```

Beginner explanation:

```text
view  = look at it
edit  = change it
take  = keep it
share = multiple owners
```

The compiler guarantees:

```text
1. Data cannot disappear while being used.
2. Two writers cannot modify the same value simultaneously.
3. Readers cannot observe partially-written state.
```

---

# Capability Ordering

Capabilities form a hierarchy:

```text
view < edit < take < share
```

The compiler must always choose the weakest capability that safely supports the function body.

Never choose a stronger capability merely for optimization.

Semantic correctness comes first.

Optimization comes second.

---

# Function Capability Inference

Users may omit capabilities entirely.

Example:

```jet
proc heal(player: Player, amount: Int) {
    player.hp += amount
}
```

Compiler infers:

```jet
proc heal(player: edit Player, amount: view Int)
```

---

# Inference Rules

Rule 1

If a parameter is only read:

```jet
proc print_name(player: Player) {
    print(player.name)
}
```

Infer:

```jet
player: view Player
```

---

Rule 2

If a parameter or any reachable field is modified but does not escape:

```jet
proc heal(player: Player) {
    player.hp += 10
}
```

Infer:

```jet
player: edit Player
```

Mutation does not imply ownership transfer.

---

Rule 3

If a value escapes the function:

```jet
saved.push(player)
```

or

```jet
return player
```

or

```jet
closure.capture(player)
```

Infer:

```jet
player: take Player
```

---

Rule 4

If multiple owners are required:

```jet
texture used by many sprites
```

Infer or require:

```jet
share Texture
```

---

# Explicit Capability Syntax

Experts may always override inference.

Examples:

```jet
proc draw(scene: view Scene)

proc heal(player: edit Player)

proc add(player: take Player)

proc cache(texture: share Texture)
```

Explicit capability annotations are promises.

The compiler must enforce them.

Example:

```jet
proc inspect(player: view Player) {
    player.name = "Kai"
}
```

Error:

```text
Cannot edit a value declared as view.
```

---

# Package Types

Jet supports:

```text
exe
lib
```

---

# Executable Packages

Executable packages should prioritize ergonomics.

Default:

```text
infer everything
```

Examples:

```jet
proc update(player: Player)
proc render(scene: Scene)
```

Capabilities inferred automatically.

Explicit capability annotations remain available but are optional.

---

# Library Packages

Library packages should prioritize API stability.

Default behavior:

```text
infer everything
```

Published packages emit inferred API metadata.

Example:

Source:

```jet
pub proc heal(player: Player) {
    player.hp += 10
}
```

Generated API metadata:

```text
heal(player: edit Player)
```

Consumers compile against the published capability metadata.

Because Jet packages are hash-pinned, changing inferred capabilities is not a safety issue.

It is a versioning issue.

Existing users remain pinned to the previous package hash.

---

# Stable API Mode

Optional:

```jet
package api = stable
```

Compiler records public capability signatures.

Example:

```text
heal(player: edit Player)
```

Future changes may trigger API break diagnostics.

---

# Explicit API Mode

Optional:

```jet
package api = explicit
```

All public functions must declare capabilities.

Example:

```jet
pub proc write(file: edit File)
```

---

# Internal Compiler Representation

Capabilities lower to implementation-specific ownership mechanics.

Users should not be required to understand these details.

Conceptually:

```text
view  -> immutable view/reference

edit  -> exclusive mutable view/reference

take  -> ownership transfer/move

share -> shared ownership
```

The source-level capability remains authoritative.

---

# Copying

Jet must never silently duplicate expensive values.

If duplication is required:

```jet
copy texture
```

If shared ownership is desired:

```jet
share texture
```

Ownership movement should be visible and intentional.

---

# Diagnostics

Diagnostics should teach capability language.

Preferred terminology:

```text
view
edit
take
share
value escapes
this function keeps the value
shared ownership
```

Avoid beginner-facing terminology such as:

```text
borrow checker
lifetime
&T
&mut T
```

---

# Required Examples

Read-only:

```jet
proc print_name(player: Player) {
    print(player.name)
}
```

Infer:

```jet
player: view Player
```

---

Mutable:

```jet
proc heal(player: Player) {
    player.hp += 10
}
```

Infer:

```jet
player: edit Player
```

---

Ownership:

```jet
proc add_member(party: Party, player: Player) {
    party.members.push(player)
}
```

Infer:

```jet
party: edit Party
player: take Player
```

---

Post-take error:

```jet
player := Player{}

party.add(player)

print(player.name)
```

Diagnostic:

```text
player was taken by party.add

Suggestions:

- use copy player
- use share player
- use player before the call
```

---

Final Goal:

Jet code should look like:

```jet
heal(player)
draw(scene)
party.add(player)
```

while the compiler automatically derives:

```text
view
edit
take
share
```

and enforces memory safety with minimal user-facing complexity.
