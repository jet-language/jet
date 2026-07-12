# Language Spec (living document)

Behavior described here is authoritative when ratified in
docs/spec/syntax-decisions.md (enforced by `tests/decisions.rs` on every
`cargo test`). Open decisions in docs/spec/syntax-decisions.md are not implemented until
ratified. The examples/ directory is the executable form of this spec: if
the spec and a passing example disagree, the spec is wrong — fix the spec.

## M1 — what exists today (values, expressions, control flow)

### Lexical rules

- Source is UTF-8. Identifiers: a letter or `_`, then letters, digits, `_`.
- Source files use the `.jet` extension (N2). The path-accepting commands
  (`jet run`/`build`/`check`/`eval`) make the extension optional: `jet run
  examples/test` resolves to `examples/test.jet` when the literal path has no
  matching file. If neither the literal path nor `<path>.jet` exists, the
  original name is kept so the file-not-found diagnostic names what you typed.
- Line comments: `//` to end of line (S5). Block comments: `/* … */`, which
  nest (an unbalanced `/*` is E0002), so any region can be commented out (S5).
- String literals: `"..."` on a single line. Escapes (S20): `\n` `\t` `\"`
  `\\` only; anything else after `\` is E0001. Interpolation (S8): `{expr}`
  embeds any printable expression; `{{` and `}}` write literal braces; a
  lone `{` or `}` is E0001.
- Multi-line strings (S70): `"""…"""` span multiple lines with the same escapes
  and interpolation. The newline right after the opening `"""` and the one right
  before the closing `"""` are dropped, and the closing `"""`'s indentation is
  stripped from every line (Swift-style). An unterminated `"""` is E0002.
- Numbers (S67): decimal `Int` (64-bit signed, E0007 if too large) and `Float`
  (digits `.` digits, optional `e`/`E` exponent). `_` digit separators are
  allowed anywhere among the digits (`1_000_000`); base prefixes `0x`/`0o`/`0b`
  give an `Int` (`0xFF`, `0o755`, `0b1010`), and a prefix with no digits is
  E0001. Unary minus is an operator, not part of the literal.
- `true` and `false` are `Bool` literals.
- Source has no visible statement separators. The lexer inserts internal
  terminators at line ends after statement-ending tokens (S6-R).
- The lexer recovers from bad characters and keeps going; one run reports
  every lexical error it can.

### Grammar (EBNF)

```
program  = { func | struct | const } ;
func     = [ "pub" ] "fn" ident "(" [ params ] ")" [ "->" type ] block ;
params   = param { "," param } ;
param    = ident ":" [ "~" | "^" | "&" ] type ;
block    = "{" { stmt } "}" ;            // S3: curly braces
// S6-R: no visible `;` — the lexer inserts a synthetic terminator (NL below)
// at each line end after a statement-ending token; the grammar stays
// terminator-based. A leading `.` or binary/logical operator on the next line
// suppresses insertion (continuation). `-> Type` / `{` must stay on the `)`
// line (E0986). `NL` below denotes that synthetic terminator.
stmt     = binding | assign | if | loop
         | "break" NL | "continue" NL | "return" [ expr ] NL
         | expr NL ;
binding  = [ "#Track" ] ( ident "::" expr     // inferred immutable
         | ident ":=" expr                    // inferred mutable
         | ident ":" type "::" expr           // explicit immutable
         | ident ":" type ":=" expr ) NL      // explicit mutable
         | destructure ( "::" | ":=" ) expr NL ;
destructure = ".{" ident { "," ident } [ ", .." ] "}"   // S74: struct fields
            | "[" [ ident { "," ident } ] "]" ;    // S74: list elements
assign   = ident ( "=" | "+=" | "-=" | "*=" | "/=" | "%="
                 | "&=" | "|=" | "^=" | "<<=" | ">>=" ) expr NL ;
// D-IF1: `if` is the one branching keyword. Two forms by body shape:
if       = "if" cond block { "else" "if" cond block } [ "else" block ]   // two-arm
         | "if" subject "==" "{" { arm } [ "else" "->" arm-body ] "}" ;  // multi-arm dispatch
arm      = arm-head "->" arm-body NL ;
arm-head = value | range | condition ; // bare value ⇒ `subject == value`; range `lo..hi` ⇒ membership (D-PATR/D-RANGE1); else a Bool condition (D-IF2 Q3)
range    = expr ".." expr ;            // inclusive (S22); no `..=` (E0318), no `step` in arm head (E0319)
arm-body = block | stmt ;        // `{ … }` block or one braceless statement (D-IF2 Q2)
loop     = [ ident "@" ] loop-body ;            // D-LABEL1: optional `name@` label
loop-body= "loop" block
         | "loop" cond block
         | "loop" ident "in" expr [ ".." expr [ "step" expr ] ] block ;
break    = "break" [ ident "@" ] NL ;           // D-LABEL1: `break name@` targets a label
continue = "continue" [ ident "@" ] NL ;        // D-LABEL1: `continue name@`
cond     = expr | "(" expr ")" ;                     // S68/D-SG2: optional parens, fmt strips them
if-expr  = "if" cond value-block "else" ( if-expr | value-block ) ;  // S68/D-SG2: value form
value-block = "{" { stmt } expr "}" ;
expr     = precedence climbing over:
           "||"  >  "&&"  >  "==" "!=" "<" ">" "<=" ">="
           >  "|"  >  "^"  >  "&"  >  "<<" ">>"
           >  "+" "-"  >  "*" "/" "%"  >  unary "-" "!"
           >  call | ident | literal | "(" expr ")" ;
```

### Semantics

- Types: `Int`, `Float`, `Bool`, `String`. Local inference: annotations on
  bindings are optional; mismatched annotations are E0108.
- A program must define `fn run` with no parameters and no return type,
  `fn run() -> Void ?` for top-level error propagation, or a single typed CLI
  parameter as described by D-CLIFLAG1 (E0101, E0122, E1308). Execution starts
  there. `run` never takes `pub` (S12).
- `name :: value` and `name: Type :: value` are immutable; `name := value` and
  `name: Type := value` are mutable (D-BIND4).
  Assigning to an immutable binding is E0111.
  Names may not shadow an existing name in scope (E0118).
- `#Track name :: value` / `#Track name := value` opt a binding into
  D-PROVENANCE1 provenance. Today this records Float binding origins for
  `value.origin() -> String`; untracked Floats return `"untracked"`.
- Arithmetic: `+ - * /` on `Int` and `Float` (never mixed — E0109);
  `% & | ^ << >>` on `Int` only. `+` on `String` is a teaching error
  pointing at interpolation. Compound assignment (S17) mirrors the binary
  operators.
- Comparisons (`== != < > <= >=`) need matching operand types and yield
  `Bool`; `&& || !` operate on `Bool` (E0110).
- `&&` and `||` combine `Bool` expressions only (D-S25-RETIRE1). Value
  alternatives in arm heads use single `|`.
- `if`/`else if`/`else` (conditions must be `Bool`); `loop` in three forms:
  `loop { }` (infinite), `loop cond { }` (conditional), `loop x in a..b { }`
  (iterates a through b **inclusive**, S22; S19-amend); `break`/`continue`
  inside loops only (E0115, S23). A loop may
  carry a suffix `name@` label (D-LABEL1) — `outer@ loop … { }` — and
  `break outer@` / `continue outer@` target it from a nested loop (E0987 names
  an out-of-scope label; E0988 flags a retired prefix label).
- `if subject == { head -> { ... } else -> { ... } }` (D-IF1/D-IF3) tests arm
  heads top to bottom. Bare values and ranges compare against the subject;
  predicate heads are `Bool`; `else` is mandatory unless enum/option
  exhaustiveness proves coverage.
- **Range arms (D-RANGE1/D-PATR, c25):** in multi-arm `if`, an arm head that is
  a range `lo..hi` fires when the subject is in that inclusive band (S22) —
  `90..100 -> "A"` desugars to `subject >= 90 && subject <= 100`. The subject
  and bounds must share an ordered scalar type (`Int`/`Char`); the open
  `Int`/`Char` domain always still needs a trailing `else` (D-PATR). c25 adds
  the porting-hazard teaching errors: `..=` in an arm head is **E0318** (Jet's
  `..` is already inclusive — write `lo..hi`), `step` in an arm head is
  **E0319** (`step` is a loop modifier, not a band), and an inverted/empty band
  `hi..lo` is **E0316**.
- **Prelude (D-PRELUDE1 = B):** `print` and `input` are ambient — usable in
  any Jet file with no `use` line. `eprint`, `args`, and `read_all_input`
  stay qualified behind `use core.io`. A user-defined function named `print`
  or `input` shadows the prelude one in that scope (prelude is lowest-priority).
  **`#NoPrelude` (D-PRELUDEX1=A)** opts a file out of those ambient names —
  call `io.print` / `io.input` after `use core.io as io`, or remove the marker.
  Libraries cannot inject into the no-prefix surface.
- `print(x)` is built in (S9); takes exactly one printable argument
  (E0103, E0112) and writes it with a trailing newline. `Float` always
  prints a decimal part (S21): `-5.0`, not `-5`.
- `input()` / `input(prompt)` is prelude (D-PRELUDE1); reads a line from
  stdin, strips the trailing newline, and returns `Result(String, IoError)`.
  Use `??` to unwrap or handle the error.
- Functions: multi-argument calls, checked arity (E0104) and argument
  types (E0112). A function with a return type must return on every path
  (E0114). Unknown names are E0102/E0107 with did-you-mean suggestions.
- **Named args and defaults (S61, D-NARG1):** parameters may carry a
  default value (`fn f(x: Int =  0)`); call sites may use a label to
  document intent (`f(x: 1)`). Labels must match the parameter name at
  that position — they never reorder arguments. Trailing defaults fill
  when omitted. Both rules apply equally to free functions **and methods**
  (D-NARG1). `jet fmt` preserves call-site labels as written (D-NARG2).
  A positional `Bool` parameter on a `pub` fn or `pub` method triggers the
  advisory L2401 lint.
- Definitions are unique (E0105), can't shadow built-ins (E0106), and
  unknown type names are E0119.

### Staged errors

Features that exist in the roadmap but not the language yet fail with an
error naming the milestone (see staged table in docs/spec/syntax-decisions.md).
A future feature must never die as a generic syntax error. Old Jet and foreign
syntax teaching is paused until post-Epoch 6 (D-S14-PAUSE); active docs and
fixtures use canonical syntax only.

## M2 — ownership (memory model v5, D-MEM1, done 2026-07-04)

Borrow-checker mechanics live in the transpiler; tier-1 users never write
Rust's `&`, `&mut`, `*`, or lifetime parameters. Two sigils, enforced (no
inference, no elevation):

| You write     | It means                       | Compiles to Rust |
|----------------|--------------------------------|-------------------|
| `fn f(x: T)`   | read (default; the only unmarked meaning) | `x: &T`   |
| `fn f(x: &T)`  | write — exclusive edit access   | `x: &mut T`       |
| `fn f(x: ^T)`  | take — ownership moves to callee | `x: T`          |

An unmarked parameter is **always** read — a body write to it, or handing it
to a `&`/`^` position, is a hard error at the definition (fix-it: add `&` at
the parameter and every call site). Call sites mirror the parameter's sigil:

```jet
fn bump(n: &Int) { n += 1 }
fn archive(name: ^String) -> String { return name }

fn run() {
    score: Int := 41
    bump(&score)                 // & mirrors &Int
    saved :: archive(^"vault")   // ^ mirrors ^String
}
```

(examples/features/memory/ownership.jet) Method receivers carry the sigil on
`self`; plain `self` is read; the sigil lives on the definition, not the call
site:

```jet
impl Player {
    fn show(self) -> Int { return self.hp }                     // read receiver
    fn heal(&self, amount: Int) { self.hp = self.hp + amount }  // write receiver
}
```

```jet
p.heal(10)    // clean — the &self is on the method definition, not here
p.show()      // plain read receiver
```

A write through a read receiver is **E0205** ("write the receiver as
`&self`"); calling a `&self` method needs a changeable binding at the call
site (**E0202**, "does not have edit access (`&`)"). Using the same name
twice in one call while a `&` on it is active is **E0204** ("while something
is being changed, nobody else may be looking at it") — pass `&x` once, or
`copy x` first.

**Named binding vs. temporary.** Passing a *named binding* to a `^` (take)
parameter without `^` is **E0209** — a hard error, never a silent clone (the
old `L0201` lint that auto-cloned is gone). A *temporary* — a literal,
`copy x`, or a call result — passes freely with no `^`, since nothing survives
to be used after. `copy x` (D-CAP2) is the one copy spelling — a real prefix
expression, not a method: `.clone()` is not user-typable Jet syntax (`clone`
falls through to the ordinary "no such method" error). `copy` on a value Jet
can't duplicate — a function, a trait value — is **E0211**; on a scalar it's
legal but redundant (already trivially copyable).

```jet
name: String :: "vault"
saved :: copy name    // fresh, independent value; `name` still usable after
```

(examples/features/memory/copy_verb.jet)

### Second-class borrows

There is no first-class (storable, returnable) borrow in v1: `-> &T` return
types and `&T` struct fields are not in the grammar (ordinary syntax errors,
no special teaching text — the mechanism is gone, not disallowed). A struct
field always owns its value:

```jet
struct Span { text: String, meta: String }

fn describe(source: String, kind: String) {
    s: Span :: Span.{text: source, meta: kind}   // fields own their data
    print(s.text)
}
```

(examples/features/memory/ref_field.jet) When a program genuinely needs
"many owners, one value," reach for `Shared<T>` or `Pool<T>`/`Id<T>` (below)
instead of a stored reference.

### Zero-copy string views

`String.trim()`/`.after(sep)`/`.before(sep)` bound to a local return a
zero-copy view into the receiver's own buffer — a real `&str` borrow,
invisible in the type (`String` stays one Jet-level type end to end) —
whenever sema can prove the binding can't outlive its owner:

```jet
padded := "  nate@jet.dev  "
email :: padded.trim()
domain :: email.after("@")
print("padded still readable: {padded}")   // reading the owner still works
```

(examples/features/memory/string_view.jet) A view's legal surface is
narrow: chain another `.trim()/.after()/.before()`, interpolate it
(`"{domain}"`), or `copy` it into an owned `String`. Any other use —
return, rebind, struct field, call argument, list/tuple element, any other
method — is **E2307**. `[T]` list slices (`list.view(a..b)`, D-DYNARRAY1,
predates this migration) follow the same owner-outlives-view reasoning,
reported as **E2305**. Either kind of view crossing a `tasks.spawn`/
`Sender.send` boundary is reported once, as **E1102** (unsendable value) —
a task or channel moves owned data between threads, and a view can't cross
without ownership.

### Escape hatches — `Shared<T>` and `Pool<T>`/`Id<T>`

Two named mechanisms cover what a first-class borrow used to promise —
share-across-scopes and many-owners-one-value — without reviving stored
references.

**`Shared<T>`** (D-SHARED-API1) is a lock-guarded shared handle — "a
copyable door":

```jet
config :: Shared.new(AppConfig.{ name: "jet-server", hits: 0 })
t1 :: tasks.spawn(() => handle(1, config))   // no `take` needed
label :: config.read(c => c.name)
config.edit(c => { c.hits += 1 })
```

(examples/features/memory/shared_config.jet) `Shared.new(x)` infers `T` from
`x`; `.read(f)`/`.edit(f)` run a closure against a read- or write-locked view,
the lock scoped to the call only. Cloning `Shared<T>` is always a cheap
handle clone, never a deep copy of `T` — so it crosses a `tasks.spawn`
boundary with no `^`.

**`Pool<T>`/`Id<T>`** (D-POOLID-API1) is a generational arena: every value
lives in one shared table, and other values point at it by `Id<T>` — plain
copyable, comparable index+generation data, never touching `T` itself:

```jet
world := Pool<Player>.new()
kai :: world.add(Player.{ name: "Kai", hp: 100, attack: 15, target: None })
world[kai].target = Val(rem)          // nested write through a real place
fallen :: world.remove(kai)           // T?, mirrors Map.remove
```

(examples/features/memory/entity_world.jet, entity_tree.jet) `pool[id]`
indexes for read and write; `.ids()` walks every live entry. A stale `Id<T>`
(its slot was removed) panics at runtime, mirroring the array-out-of-bounds
precedent (examples/features/memory/pool_stale_id.jet) — not a new
diagnostic code.

### `policy no_alloc`

A bare module-level `policy no_alloc` (D-NOALLOC-SEM1) flags four
allocation-shaped expressions written directly in that module's own function
bodies — string interpolation with a `{…}` hole, `.push`/`.insert`, a
struct/enum literal for a heap-owning type, `copy` of a heap-owning type —
as **E0921**. The check is local only: a call into another function is that
function's own module's problem, never followed.

```jet
policy no_alloc

fn integrate(e: &Entity, dt: Float) { e.pos += e.vel * dt }
```

(examples/features/memory/no_alloc_policy.jet)

`const NAME = value` always looks the same; the transpiler emits Rust
`const` or `static` when the address is taken or the type needs it.

Aliasing rule, stated for humans: *while something is being changed, nobody
else may be looking at it.* Foreign `read`/`write` spellings are paused under
D-S14-PAUSE and get ordinary parse errors.

## Access capability sigils (D-MEM1)

The capability is a prefix sigil on the **type**, not the name. Two sigils
ship in v1 (unmarked read is the default, not a sigil):

| Sigil | Capability | Compiles to Rust |
|-------|-----------|-------------------|
| `T` (bare) | read — callee only reads; enforced, never elevated | `x: &T` |
| `&T` | write — exclusive edit access | `x: &mut T` |
| `^T` | take — ownership moves to callee | `x: T` |

`~` is not part of the v5 grammar (ordinary syntax error). Raw-pointer access
(`p.*` postfix deref, prefix `*x`) is a separate, `#Unsafe`-gated mechanism
(D-CAP9) — not a parameter capability; the compiler's `AccessConvention`
enum keeps dead `Share`/`Raw` variants internally, inert until a future
tier reactivates them.

### Placement

Capability rides the type on the parameter:

```jet
fn damage(p: &Player, amount: Int) {   // &Player: write; Int: read (bare)
    p.hp = p.hp - amount
}
```

The call site mirrors the sigil — the capability is always visible where
mutation or movement happens:

```jet
damage(&p, 30)    // & mirrors the parameter's &Player
close(^file)      // ^ mirrors ^File — file is consumed
```

### Optional composition

A capability sigil composes with `?` (optional presence) directly: `&User?`
means "write access over an optional User", `^Texture?` means "take an
optional Texture". The sigil and `?` follow the same type-side grammar as
any other type annotation — the sigil is the parameter prefix, `?` is the
type suffix.

### E0029 — two capability markers

Placing more than one capability sigil on a single parameter is a parse error:

```
error[E0029]: two capability markers on one parameter
  --> file.jet:3:12
   |
 3 | fn bad(p: &^Player) { … }
   |           ^^ remove one capability marker
```

Access capabilities use sigils only: bare `T` (read), `&T` (write), `^T`
(take) — no fourth spelling.

## M3 — data & methods (done)

Structs and enums carry fields; methods attach behavior (S27). Ratified
surface (Group 2): struct literals **`Type.{f: v}`** (S29; flush, S29-FLUSH; dot-prefixed by D-DOTCTOR2); enums with
**`Type.Variant`** (S30); **`==` pattern tests** (S31); optional
**`T?`** with **`Val(v)`** / **`None`** (S32); generic args
**`Type<Args>`** (S33). `None` is only legal for `T?`, never plain `T`.

```
struct Circle {
    radius: Float;

    fn area(self) -> Float {
        return 3.14159 * radius * radius;
    }
}

impl Circle {
    fn unit() -> Circle {
        return Circle.{ radius: 1.0 };
    }
}
```

- **`self`** is the receiver; prefix the type sigil (`^self`, `&self`) like any parameter (D-MEM1) — bare `self` is read.
- **Self-mutation (D-MUTSELF1):** inside a **`&self`** method the receiver may be
  changed in place — assign a field (`self.field = v`), update one (`self.field += v`,
  S17), or reassign the whole receiver (`self = New.{…}`). No new syntax (a `&`
  parameter is already a valid assignment LHS). The same write in a non-`&self`
  method (a read receiver) is **E0205**, pointed at the assignment with a "write
  the receiver as `&self`" fix. Calling a `&self` method needs a changeable
  receiver binding (`:=`), enforced at the call site by E0202.
- Invoke with **`c.area()`** (not `area(c)`).
- Methods may live **inside** the type, in **`impl Type { }`**, or as a top-level
  external inherent method **`fn Type.method(self, ...) { }`** (D-EXTMETH1) —
  same rules either way. The type must be defined in the current source module.
- Static methods omit `self` (e.g. `Circle.unit()`).
- **Named constructors (D-CTOR1):** multiple construction shapes = multiple
  distinctly-named no-`self` statics returning the type (`Point.cartesian`,
  `Point.polar`). Overloading is rejected; a duplicate name is E0105 with
  a teaching message pointing at constructor naming.
- Enum `if subject == { … }` arms must be exhaustive; missing cases are a compile error.
- **Traits (S28, M9):** `trait Name { fn sig(self) -> T; … }` — signatures
  only. Implement inside a type (`impl Trait { … }`) or outside as
  `impl Type.Trait { … }` (qualify foreign types: `impl other.Point.Shape`).
  A trait name in type position (`[Shape]`, `fn f(s: Shape)`) means
  dynamic dispatch with invisible boxing. Generic params: `fn f<T: Bound>(…)`
  and `struct Pair<T> { … }`. Built-in traits follow S55: auto
  `Printable`/`Equatable`; explicit `@[Comparable]`, `@[Codable]`,
  `@[Encode]`, `@[Decode]`.
- **Encoding traits (D-SERDE2/D-SERDE16):** `Encode.encode(self) -> DataTree`
  and `Decode.decode(tree: DataTree) -> Self ? DecodeError` are ordinary Jet
  trait methods. `DataTree.decode<T>()` is the one public typed-dispatch path;
  primitive, container, generated, and hand-written implementations all use it.
  Built-in derives generate Jet source fragments beside the marked type, then
  run those fragments through the normal parser, sema, TIR, and codegen pipeline.
  A user-defined derive may expand only when its provider or target type is
  entry-local; otherwise E2711 points at the derive marker.
- **Tags (D-QUAL2):** `tag Name;` or `tag Name { }` — a marker qualifier with
  no methods that erases at runtime (codegen emits nothing). Tags are the second
  and only other qualifier kind beside traits; the beginner rule is one
  sentence: *methods → trait, no methods → tag.* A tag carries no methods, so
  declaring one in a tag body is **E0732**, and using a tag where dispatch or
  method attachment is expected — `derive`d, or implemented/used as a trait —
  is **E0731** (fix-it: declare it as a `trait`). All tags are PascalCase
  (D-CASING1).
- **Markers (D-ATTR1/D-ATTR2/D-MARKER-CANON1):** `#Marker` or `#[A, B]` on the
  line before a declaration. Block markers use PascalCase and parenthesized
  arguments when arguments exist. `@Pure fn` is a prefix marker; `comptime`
  stays a prefix keyword.
- **Statement switch attributes (D-CANVASSTATE1):** `#Off <stmt>` parses and
  type-checks one statement, including block-shaped statements, then emits no
  code in every build. `#DebugOnly <stmt>` parses and type-checks the statement
  in every build, emits only in debug/dev builds, and strips from release output.
  Names introduced inside either marker are scoped to that marker body.
  `build.profile` is not a user-typeable comptime value.
- **Canvas metadata (D-CANVASMETA1):** `#Meta(category: "Movement", tunable)`
  attaches checked tooling facts to bindings, top-level consts, and functions.
  `category` must be a non-empty plain string literal; `tunable` is a bare flag.
  The marker emits no code and changes no runtime behavior.
- **OS-target gating & dispatch (D-OSTARGET1/D-OSTARGET2):** `#Target(Os.Linux
  |Macos|Windows)` gates one `impl` block to a native OS; `jet build
  --target=<triple>` emits only the matching build's impls (host OS by default).
  Ungated code reaches the surviving impl through the compile-time switch
  **`comptime if build.os == { .Linux -> … .Macos -> … .Windows -> … [else -> …]
  }`** — `build.os` is a compiler-known comptime value, the switch folds to the
  arm matching the build's target OS and discards the rest before any gating
  check runs. Arms must cover every OS or carry an `else`
  (**E-OSTARGET-DISPATCH-EXHAUSTIVE**); the subject must be `build.os`
  (**E-OSTARGET-BUILD-CONTEXT**); arm heads are bare OS variants
  (**E-OSTARGET-DISPATCH-ARM**). See syntax-decisions.md → D-OSTARGET2 for the
  full rules.
- **Build-time embedding (D-CTIO1/D-CTFIND1/2):** inside a `comptime` binding,
  **`embed_file("path") -> String`** bakes a file's UTF-8 text into the binary
  and **`embed_bytes("path") -> [U8]`** bakes its raw bytes (binary-safe, no
  UTF-8 requirement — images, fonts, any blob). **`find("glob") -> [String]`**
  returns sorted relative file paths for a std-only glob (`*`, `**`, `?`,
  `{a,b}`, `[a-z]`). These are the *only* sanctioned build-time I/O; comptime is
  otherwise pure (**E0951**). Paths/globs must be string literals resolved
  relative to the embedding file's directory, never absolute and never escaping
  the project via `..` (**E0957**). A missing or unreadable embedded file is
  **E0955**; for `embed_file`, a non-UTF-8 file is also **E0955**, with a fix
  pointing at `embed_bytes`. Every embedded file and every file matched by
  `find` records its sha256 in `.jet/lock`.
- **Published schema migrations (D-MIGRATE1/D-MIGRATE2):** `@PublishedSchema struct
  Name { ... }` marks a public record whose field layout is snapshotted at release
  under `.jet/cache/schema/`. On later project builds, sema compares the current
  shape to the saved snapshot (keyed by field name, so order is ignored). A
  breaking data-shape change is refused — **E0910** — unless a `migration` op
  declares the intent. The four ops:

  ```jet
  migration UserRecord {
      rename name -> display_name              // D-MIGRATE1: field renamed (same type)
      remove legacy_id                         // D-MIGRATE2D: field deleted
      add verified: Bool =  false               // D-MIGRATE2A: new field + default for old data
      change price: Int -> Usd via { (c) => Usd(c) }   // D-MIGRATE2E: type change + converter
  }
  ```

  - `rename` must target an existing field with the same type.
  - `change f: Old -> New` resolves its converter in order (D-MIGRATE2B): the inline
    `via { … }`, else an `impl Old -> New` in scope (the D-ERR-CONV surface), else
    E0910 asking for one. The `via` body is single- or multi-line and reuses the
    `->` arrow and lambda grammar.
  - `add f: T =  default` supplies the value old records (written before the field
    existed) are read with. A field is only "added" if absent from the snapshot.
  - There is **no `reorder` verb** (D-MIGRATE2F): reordering is never a breaking
    change and needs no op (writing `reorder` teaches E0911).
  - `drop` is not a verb (use `remove`); both `drop` and `reorder` are taught back
    via **E0911**, as is any other unknown verb.

  A declared op that contradicts the real shape (e.g. `remove f` where `f` still
  exists, `add f` where `f` already existed, a `change` whose from/to types don't
  match) is itself an E0910-family teaching error. E0910 checks *intent*; the
  runtime data conversion is the D-MIGRATE4 chain below.
  Single-file runs accept the marker but only enforce the check when a project
  snapshot exists.

  **`jet inspect schema` (D-MIGRATE2C):** `jet inspect schema status` lists every snapshotted
  `@PublishedSchema` type with its pinned published version and fields, flagging any
  type that has a pending breaking change vs its snapshot (reusing the E0910 diff).
  `jet inspect schema squash --before <ver>` re-baselines: it rewrites each snapshot to the
  *current* struct shape and records `squashed_before = <ver>`, so future builds
  treat the current shape as the authoritative baseline and migration blocks for
  versions before `<ver>` are no longer required (delete the now-stale blocks). It
  edits only `.jet/cache/schema/`, never user source. There is **no `jet inspect schema
  check` verb** — `jet build`'s E0910 is already the CI gate.

  **Decode-time migration transparency (D-MIGRATE3=A):** `decode_traced<T>(raw)
  -> DecodeResult<T> ?` sits beside `decode<T>` on every codec that shares this
  decode machinery (json/csv/toml/yaml, D-ENC1). `DecodeResult<T>` is `{ value:
  T, migration: MigrationStatus }`; `MigrationStatus` carries `.migrated: Bool`,
  `.from` (the source shape's version label), and `.steps` (one entry per
  migration step applied, `"v1->v2"` style). `decode` itself is unchanged —
  same call, same cost, for anyone not asking (I8). `.migrated` is `false` and
  `.from`/`.steps` are empty for a plain type and for a `@PublishedSchema`
  type decoding data already shaped like the current struct.

  ```jet
  r    :: json.decode_traced<UserRecord>(raw)?
  user :: r.value
  if r.migration.migrated {
      log.info("record {user.id} arrived as schema {r.migration.from}")
  }
  ```

  **Runtime migration chain (D-MIGRATE4=A):** decoding a concrete
  `@PublishedSchema` type that derives `Decode` and has `migration { }` blocks
  runs the chain. The blocks, in source order, are the steps: with `K` blocks
  the historical shapes are `v1` (oldest) … `vK`, and the current struct is
  `v(K+1)`; each historical shape's field set is derived at compile time by
  inverting the ops (`add` ⇒ absent before, `remove` ⇒ present before,
  `rename a -> b` ⇒ `a` before, `change` ⇒ no field-set difference). At decode
  time:

  1. **Current shape first** — the ordinary decode is tried as-is. Success is
     the fresh case (`migrated: false`). This is also the ambiguity rule:
     *prefer the newest matching version*, so data that satisfies the current
     shape never migrates.
  2. **Shape detection** — on failure, the data's top-level field-name set
     (wire keys, after any `#[Rename]`/`#[RenameAll]` treatment) is compared
     against the historical shapes, newest (`vK`) to oldest (`v1`); the first
     match wins.
  3. **Walk forward** — the matched shape's data is rewritten step by step,
     oldest-matching → current: `rename` moves a key, `remove` drops one,
     `add` evaluates its default expression and fills the field, `change`
     decodes the old field type, runs the `via { … }` converter (or the
     `impl Old -> New` conversion, D-MIGRATE2B), and re-encodes the result.
     Converter bodies and `add` defaults are ordinary Jet expressions,
     type-checked and lowered through the normal pipeline. The rewritten data
     then decodes as the current shape.
  4. **No match** — the ordinary decode error is returned unchanged (garbage
     stays garbage).

  Plain `decode` applies the same chain silently; `decode_traced` reports it
  (`from: "v1"`, `steps: ["v1->v2", "v2->v3"]`, …). Version labels are
  positional — `v1` is the oldest shape the blocks describe. Types without
  migration blocks pay nothing: no step functions and no per-type chain code
  are emitted, and their decode path is unchanged. CSV applies the chain per
  row (an old-header file migrates every row; the batch-level status reports
  the first migrated row).

- **Struct layout control (D-REPRC1):** `#Layout(c)` before a struct stamps
  `#[repr(C)]` on the generated Rust struct, enabling direct C-FFI pointer
  sharing. Field order is preserved as written. Growable fields (`[T]`, `[K: V]`,
  `String`) are rejected with **E1104** because they lack a stable C layout;
  fixed-size arrays `[T#N]` are allowed. Reserved variants (`packed`, `align(N)`,
  `columnar`) parse but error with **E1105** until their milestones ship.

## M4 — errors as values (done)

Fallible functions return **`T ? E`** (S34): `T` is the success payload,
`E` is any enum, struct, `String`, or the default **`Error`** type. Omitting
the error side in a function return — **`T ?`** — means **`T ? Error`**.
Build outcomes with **`ok(v)`** and **`err(e)`**; test them with
**`== ok(n)`** / **`== err(e)`** (same pattern machinery as M3 optionals).
Cross-type **`?`** conversion supports two forms:
- **`Fallible`** trait (D-ERR2): `impl MyFail.Fallible { fn to_error(self) -> Error { … } }` — converts any typed error to the universal `Error`. Prelude types implement `Fallible` by default.
- **Declared typed conversion** (D-ERR-CONV): `impl Source -> Target { return Target.Variant(self) }` — converts a `Source` error into a typed `Target` error; `?` applies it automatically. Declared once per (Source, Target) pair; rejected unless declared (orphan rule S28 applies). `E2404` fires when `?` would need an undeclared conversion; `E2405` fires on duplicate declarations; `E2406` fires on orphan-rule violations.

- Postfix **`?`** (S7) propagates: unwraps `ok`, early-returns `err`. The
  enclosing function must return a compatible fallible type. On **`T?`**,
  `?` propagates `None` when the function returns an optional.
- In a function return type, **`T?`** parses as **`T ?`** and the formatter
  writes the space. A function that returns an optional writes
  **`-> (T?)`**.
- **`?? <expr>`** (S35/S71) is the fallback operator on a fallible value or
  optional: yields the success payload or evaluates the right side. Precedence is
  looser than **`&&`** / **`||`**, so `a? ?? b` and `x == 1 || y ?? 0`
  parse predictably. The right side may be a value, **`return`**, **`return expr`**,
  or **`panic(…)`**. The retired word **`or`** is paused under D-S14-PAUSE and
  gets an ordinary parse error.
- **`panic("msg")`** and **`require(cond)`** / **`require(cond, "msg")`**
  (S36) stop the program with a friendly report on stderr and exit code 70.
- In **`if <fallible-expr> { … }`**, when the subject is not a plain
  name, **`it`** names the subject for pattern arms like **`it == ok(n)`**.
- **`fn run()`** may stay bare for beginner programs. Use
  **`fn run() -> Void ?`** only when the entry itself propagates errors with
  **`?`**; returned errors print and exit non-zero.

Unchecked fallible values (**E0401**), ignored fallible calls (**E0402**),
ignored **`@MustUse`** results (**E0419**), bad propagation (**E0403**),
`ok`/`err` outside a result context (**E0404**), and fallback type mismatches
(**E0405**) are compile errors with fixes that name **`?`**, **`??`**, pattern
tests, binding, and **`.drop("reason")`** / **`#Suppress(MustUse)`** for
intentional discard (D-IGNORERET2).

## M6 phase 1 — `jet fmt` (done)

**`jet fmt <file.jet>`** rewrites the file in place to canonical Jet style
(S44). **`jet fmt --check <file>`** prints a unified diff and exits **1**
when the file would change (CI mode). Formatting is lex → parse → print;
sema and rustc are not run.

Style (zero configuration): 4-space indent, `{` on the same line as its
header, one statement per line, at most one blank line between top-level
items, spaces around binary operators, no space before `;`/`,`/call `(`,
trailing `;` on statements (S6). **Line width is not enforced in v1.**

`//` and `/* … */` comments are preserved and re-attached by source span. Real
parse errors still block fmt.

Idempotence: **`fmt(fmt(x)) == fmt(x)`** on every `examples/*.jet` and
`tests/ui/*.fixed.jet` (`tests/fmt.rs`).

## M6 phase 2 — `jet test` + `jet new` (done)

**`#Test("name") { … }`** (S43, D-CASING1 follow-on) — top-level blocks only.
Bodies parse like a parameterless function; use **`require(cond)`** /
**`require(cond, "msg")`** and **`require_eq(a, b)`** (S36) for checks. Duplicate
test names → **E0105**; a nested `#Test` block → **E0601**; bare `test "name"` is
paused under D-S14-PAUSE and gets an ordinary parse error. **`jet run`** / **`jet build`** ignore test
blocks; only **`jet test`** compiles and runs them.

**`jet test <file.jet>`** (or a directory of `*.jet` files) builds one harness
binary per file (no cargo project; R9). Each test runs in isolation; failures
use a generated unwind boundary (not observable in user code). Output is one
line per test (`name: pass` / `name: FAIL`), a summary (`N passed, M failed`),
and exit **1** when any test fails. **`require_eq`** failures print
`left: …, right: …` on stderr.

**Scope members (D-DOTSCOPE1)** — inside a `#Test { … }` body, a
statement-position `.name { … }` / `.name(args) { … }` resolves against the
marker's declared vocabulary (`Syntax::scope_members`). This is the one spelling
for scope vocabulary (I8); the parser/sema shape is generic (a marker→members
table), so other markers can grow members later without new grammar. `#Test`
declares four; `#Bench` (and every other block marker) declares none, so a member
there is **E0614**. A member outside any member-declaring marker is **E0615**;
an unknown member **E0614** (lists the vocabulary); a wrong argument shape
**E0617**; a member nested instead of a top-level statement of the block
**E0618**. Members only *run* under **`jet test`** — `jet run`/`jet build` ignore
`#Test` blocks entirely (unchanged); a malformed member is still reported in any
mode (structural check).

- **`.setup { … }`** — must be the first statement (**E0616** otherwise). Its
  statements are spliced inline and run first; a failure inside fails the test on
  the normal path. It does **not** open a new scope — bindings made in `.setup`
  are visible to the rest of the test body (init sugar).
- **`.expect_fail { … }`** — the region *must* fail (a `require` failure or a
  panic). It runs under the harness's panic-catching boundary with a silenced
  hook; if it completes cleanly the **test** fails with `expected this region to
  fail, but it passed`. If it fails, execution continues after the region and the
  test can still pass.
- **`.timeout(<dur>) { … }`** — the region must complete within the duration or
  the test fails with a `timeout: region took …` message. v1 ships **post-hoc**
  semantics: the region runs to completion, then its elapsed time is compared
  against the budget (it does not interrupt a hang — out of scope). `<dur>` is a
  bare duration literal (`ns`/`us`/`ms`/`s`); it needs no `#UnitFamily` in scope.
- **`.skip { … }` / `.skip("reason") { … }`** — the region is **not executed**
  (emitted as a dead `if false` block, so it still type-checks). When `.skip` is
  the **first** statement the whole test is skipped: it reports `name: skip` and
  the summary gains a `, K skipped` tail (the classic `N passed, M failed` line is
  unchanged when nothing skips). A `.skip` later in the body skips only that
  region; the rest of the test still runs.

**`jet new <name>`** creates `<name>/main.jet` (hello world) and
`<name>/.gitignore` (`build/`). No manifest (M12; opt-in).

Example: `examples/features/tooling/tests.jet`; scope members in
`examples/features/tooling/test_members.jet`. Goldens: `examples/features/expected/20_tests.test.out`,
`tests/jet_test.rs`, `tests/fixtures/test_fail.jet` + `.fixed.jet`, and the
`scope_*` fixtures for the member fail paths.

## `#Bench` region benchmarks + perf timing (c121, D-BENCH1) — done

**`#Bench("name") { … }`** (D-BENCH1, D-BENCH-MARKER1) is the exact sibling of `#Test`: a
top-level block whose body parses like a parameterless function (and may use
`require`/`require_eq`). **`jet run`** / **`jet build`** ignore bench blocks. A
file with `#Bench` blocks runs per-region under **`jet bench`** — each region's
body is warmed, its iteration count auto-scaled to ≥1ms, sampled, and reported
as `name  <ns> ns/iter (±sd)  <ops> ops/sec`. A file with no `#Bench` blocks
keeps whole-program `jet bench` timing (5 warmup + 20 trials). The body call is
`black_box`'d so the optimizer can't elide it. Example:
`examples/features/tooling/bench.jet`; golden `examples/features/expected/105_bench.out`
(the `jet run` `main` output) + structural check in `tests/jet_test.rs`.

**Compiler phase timing** — set **`JET_TIMING=1`** and any build writes
`jet-timing.json` (load/sema/ffi/codegen µs + generated-Rust bytes), prints
`jet-timing binary_bytes=…` after link, and the LSP appends per-request latency
to `jet-lsp-timing.json`. All gated by the env var (zero cost otherwise; I6
hand-rolled JSON, no external crate). **`tools/perf/dashboard.sh`** aggregates a
table across representative programs; **`tools/perf/ci-perf-check.sh`** gates
against the committed **`tools/perf/baseline.json`** (sema time + binary size,
15% threshold); **`tools/perf/update-baseline.sh`** refreshes it.

**NixOS / flake:** `nix develop` provides `cargo`, `rustc`, `gcc`, `nodejs`,
and a **`jet`** wrapper around `target/debug/jet`. **`cargo build`** once, then
`jet run …` / `jet self lsp` / `cargo test --test lsp`. Editor setup:
`editors/vscode/README.md`. Release binary: `nix build .#jet`.

## Unified FFI frame (D-FFI-UNIFY1)

Every foreign ecosystem mounts through one model: a language root plus library
name, `<lang>.<lib>`, with generated bindings under `.jet/bindings/<lang>/`.
C and JS are the active namespace binders today. C uses the namespace surface
(`use c.<lib>` / `#Extern module c.<lib>`). JS uses one `use js.<lib>` surface;
the host is target-dispatched, with browser JS on web targets and the native
JS-on-WASM host on native targets. Generated JS binding caches live under
`.jet/bindings/js/`: `<lib>.jet` carries the callable Jet surface and
`<lib>.d.ts` records the TypeScript declaration provenance. Rust keeps the shipped
`extern rust "crate@version" { ... }` declaration block as its active binder
surface until the `rust.*` namespace migrates. Python and Swift roots are
registered for their ratified binders; Swift's planned route is a typed bridge
over generated C-ABI shims.

## M7 — Rust FFI (`extern rust`, done)

**`extern rust "crate@version" { … }`** (S50) declares foreign functions. Each
entry is a normal Jet signature plus **`= "rust::path"`** naming the target
item. This source-level declaration is sufficient even inside a project with
`pkg.jet`; users do not need the package manager just to call a foreign
function. **`extern rust "std" { … }`** works for Rust standard-library items with
no extra dependency. Non-`core` crates require an exact version pin (**E0701**).

Allowed boundary types pass **by value**: `Int`, `Float`, `Bool`, `String`,
`Char`, `[T]`/`[K: V]`/`T?`/`T ? E` built from allowed types, and
structs/enums whose fields are allowed. No borrowed parameters or returns, no
callbacks (**E0702**).

When any crate dependency is needed, the driver builds a hidden cached cargo
bridge under `~/.cache/jet/ffi/` and links it into the generated program (R9:
the user's folder never grows a manifest). Missing **`cargo`** → **E0703**;
fetch/build failures → **E0704** (cargo output in an indented block); a wrong
foreign path or signature → **E0705**. Panics inside foreign code are caught
at the boundary and become the M4 runtime report (exit 70).

Teaching: **`unsafe`** / C-style FFI spellings → **`extern rust`** (**E0031**).

Example: `examples/features/lowlevel/ffi.jet` (`base64@0.22`). Ui: `tests/ui/ffi_*.jet`.
Integration: `tests/ffi.rs` (gated on `cargo`).

## E2-M14 — C FFI (implemented: overlay + merge + link + bind backend)

**S59** — C import with auto-generated bindings (default) and optional user
overlay. (Full spec follows in this section.)

| Layer | Shape |
|---|---|
| Autogen | `#Bindgen module c.<lib>.__bindgen__ { … }` in `.jet/bindings/c/<lib>.jet` |
| Overlay | `#Extern module c.<lib> { … }` — empty `{ }` = no overrides |
| Call site | `use "header.h" as alias` or `use c.<lib> as alias` (one per lib per file) |

Function bodies mirror Rust FFI: `fn init_window(w: Int, h: Int, t: String) = 
"InitWindow";` (the string is the C linker symbol). On any C `use`, the compiler
loads the bindgen cache at `.jet/bindings/c/<lib>.jet` (when present), merges the
user overlay over it (**effective module = bindgen ∪ overlay; overlay wins**;
incompatible re-declaration → **E3205**), and materializes one synthetic module
so calls resolve like any namespaced module call. Codegen emits an `extern "C"`
block plus small per-function wrappers (the only place compiler-vetted `unsafe`
is emitted, S58); `String`↔`*const c_char` and `Char`↔`u32` convert at the edge.

Link key = last segment `<lib>`: a declared `<lib>: c@…` dep in the `deps:`
block of `pkg.jet` (`c@system` → pkg-config with a bare `-l <lib>` fallback;
`c@"path"` → local `-L`/`-I`/`-l`) → else `pkg-config <lib>` → **E3201**. Link flags (`-L native=…`,
`-l <lib>`) are resolved at **build time** (not during front-end checking, I3) and
threaded into the `rustc` link line. By-value scalars/`String`/C-layout
structs+enums at the edge; aggregates (`[T]`, maps, `T?`, tuples, …) → **E3203**;
pointers require `use core.mem` + `#Unsafe` (E2-M13) → **E3202** (registered;
unreachable until the pointer tier lands). `#Bindgen` is legal only inside a
generated cache file (**E3207**); users may not name the reserved `__bindgen__`
segment (**E3206**); two `use` forms for one lib in one file → **E3204**.

`jet inspect bind <header.h> --pkg <lib>` is the manual cache-refresh entry point and
shares the compile-time auto-bind backend (owner 2026-06-18: native std-only
implementation, D-CBIND3 superseded). It parses C function prototypes over the
bindable type subset (scalars, `char*` strings, `void`) and emits a `#Bindgen`
cache; declarations it cannot map are skipped and reported rather than faked
(I3). **E3208** fires only when the header cannot be read or contains no
bindable prototypes — the fix is a hand-written `#Extern module c.<lib>` overlay
for those declarations. Rust FFI (S50) is unchanged. Diagnostics:
**E3201–E3208** in diagnostics.md with snapshots (front-end ones under
`tests/ui/cffi_*`; link-time/gated ones pinned in `tests/cffi.rs`).

## E2-M13 — Expert low-level tier (S58, implemented)

C/Zig-class control behind two explicit gates; ordinary Jet never reaches it and
emits **zero** `unsafe` (the I1 amendment, D-LL1, recorded in `architecture.md`).

- **Discovery gate** — `use core.mem;` unlocks the low-level vocabulary (`*T`,
  `mem.volatile_read`, `mem.volatile_write`, `mem.address_of`, allocators).
  Naming one of these without the import → **E3102**.
- **Audit gate** — `#Unsafe("reason") { … }` opens the operations that can
  violate memory safety (pointer build/deref, volatile access). The reason
  string is the argument to `#Unsafe` itself (D-UNSAFE2; the former separate
  `#Audit("…")` line is retired → **E0055**). Under **D-UNSAFE-REASON1=B**,
  bare `#Unsafe { … }` and bare `#Unsafe fn` compile and emit lint **L3101**.
  `#Unsafe("reason") fn` marks a whole-function contract; its body is itself
  an audited region, and calling it requires an enclosing `#Unsafe` block →
  **E3103**.
- **Operations** — prefix `*x` takes a raw pointer to `x`; postfix `p.*`
  dereferences it. `mem.address_of(x)` is inert (a plain address as `Int`) and
  legal outside a gate. `mem.volatile_read(p)` and
  `mem.volatile_write(p, value)` perform explicit volatile/MMIO access through a
  typed pointer. Using a low-level op outside `#Unsafe` → **E3101**.

Codegen stays dumb (I3): an `#Unsafe { … }` region lowers straight to a Rust
`unsafe { … }`, an `#Unsafe fn` to a Rust `unsafe fn`. All gating is decided in
sema. Diagnostics **E3101–E3104 + L3101** in diagnostics.md with snapshots
(`tests/ui/lowlevel_e310*`, `tests/ui/mem_arena_gate`, `tests/ui/mem_use_after_free`,
`tests/ui_lint/unsafe_missing_audit`); the audited end-to-end example is
`examples/features/lowlevel/lowlevel.jet`.

## Web browser API (D-FLAGSHIP-WEBAPI1, implemented)

`use core.web as web` exposes the browser-owned pieces that a web flagship slice
needs outside the retained `core.ui` paint surface:

- `web.on(selector, event, handler)` binds a DOM event listener. The handler gets
  a `WebEvent` value; handlers that do not need the event may ignore it.
- `web.value(selector) -> String` reads an input value or element text.
- `web.storage.local.get(key) -> String?` and
  `web.storage.session.get(key) -> String?` read browser storage. Missing keys
  compose with the normal `??` fallback: `web.storage.local.get("tasks") ?? "[]"`.
- `set(key, value)`, `remove(key)`, and `clear()` mutate local/session storage.

`core.web` carries the `Browser` effect. The web JS backend emits real
`addEventListener`, `querySelector`, `localStorage`, and `sessionStorage` calls;
native codegen lowers the same checked calls to inert stubs so rustc never
becomes the browser API checker.

## First-party events and hooks (D-EVENT1, implemented)

`use core.event as event` exposes the first compiler-known event family as
ordinary Core values. There is no `event` declaration syntax in this slice.

- `event.new<T>() -> Event<T>` creates a typed many-subscriber occurrence stream.
- `event.with_policy<T>(policy) -> Event<T>` creates the same stream with an
  explicit sync/queued dispatch policy.
- `event.hook<T, R>(fallback) -> Hook<T, R>` creates an ordered intervention
  point. `.run(payload, fallback)` returns the last active handler result, or
  the call-site fallback when no handler is active.
- `event.scope() -> EventScope` owns subscriptions. `scope.cancel()` unsubscribes
  all owned subscriptions; `scope.active_count()` reports currently active
  subscriptions.
- `Event<T>.on(scope, handler)`, `.once(scope, handler)`, and
  `.on_priority(scope, priority, handler)` return `Subscription`. Priority sorts
  before source order; `once` auto-unsubscribes after first delivery.
- `Event<T>.emit(payload)` and `.emit_async(payload)` return `EventTrace`.
  `EventTrace.summary()` prints delivered/queued/dropped counts.

```jet
use core.event as event

fn run() {
    scope :: event.scope()
    clicked :: event.new<Int>()

    sub :: clicked.on(scope, (n) => { print("clicked {n}") })
    clicked.once(scope, (n) => { print("once {n}") })

    print(clicked.emit(1).summary())
    sub.unsubscribe()
    scope.cancel()
}
```

### Jai transliteration: compact cast/deref chains (D-POINTERCHAIN1=A, docs-only)

Jai allows a single compact expression that casts and dereferences a raw pointer
in one chain, e.g. `slot.value_pointer.(*Bool).* = true`. Jet rejects that
compact form outright — there is no cast-and-deref operator. The equivalent is
two explicit, audited lines: reinterpret an address through
`mem.Ptr<T>.from_addr(addr)` (the cast step), then read or write through it
with postfix `p.*` (the deref step), both inside `#Unsafe`:

```jet
use core.mem

flag: Bool :: true
#Unsafe("flag is live on this stack frame and the pointer never escapes") {
    addr: Int :: mem.address_of(flag)
    p :: mem.Ptr<Bool>.from_addr(addr)
    print(p.*)
}
```

No new syntax, sema, or codegen — this section only names the existing
`mem.address_of` / `mem.Ptr<T>.from_addr` / postfix `.*` vocabulary (§E2-M13
above) as the answer to "what does Jai's chain do in Jet." Example:
`examples/features/lowlevel/pointer_cast_deref.jet`.

### Allocators (D-ALLOC1, D-ALLOC-C, D-ALLOC-D; ratified 2026-06-19, implemented)

Four allocators ship under `core.mem` — `Arena`, `Bump`, `Pool`, `Fixed` — all namespaced
under `core.mem.alloc` (D-ALLOC-C). No `#Unsafe` needed; `use core.mem` is the discovery
gate (E3102). Constructors: `mem.Arena.new()` / `mem.Arena.new(capacity: N)` (D-ALLOC1);
allocate with `arena.alloc(value)`. Two lifecycle verbs (D-ALLOC-D): `reset()` keeps the
backing buffer (cheap, arena is reusable), `free()` returns memory to the OS. **E3104**
catches `alloc` on an already-`free`d arena. Example: `70_arena.jet`.

### Arena regions and scope-bound views (D-ALLOC2, D-REGION1; ratified 2026-06-21, implemented)

The c05 upgrade makes the arena *real*: `arena.alloc(value)` bump-allocates into a shared
buffer (the typed-arena pattern) and returns a **scope-bound `view`** — Rust `&'arena mut T`
— not an owned copy. The runtime (`Source/Prelude/Mem.rs`, `mod jet_mem`) carries the one
vetted lifetime-extension internal (D-LL1, inside the helper only; never leaks to user code,
golden-test enforced); `reset(&mut self)`/`free(self)` take the arena by `&mut`/value, so
rustc itself forbids reset/free while a view is live — the I2 backstop.

A view is sound only inside its **region** and only until the arena is `reset`/`free`d. Two
sema checks (`Source/Sema/CheckerOwnership.rs`), both at least as strict as rustc's borrow
checker so Jet always rejects first (I2):

- **E0631** — the view escapes its region: returned, stored in another binding
  or struct field, passed to a `&`/`^` parameter, or captured by an escaping
  closure.
- **E0632** — the view is read after its arena was `reset`/`free`d.

Regions (D-REGION1): **implicit and scope-inferred by default** — the region is the lexical
scope of the `arena` binding; the beginner never types a lifetime. **Plus an explicit
`region r { … }` block** (lowercase contextual keyword, `KW_REGION`) for the expert cases
inference can't give: a region spanning two allocators, narrower than the enclosing function,
or named. The escape rule is enforced against the inferred scope or the named region
identically. v1 restriction (I8): views are non-reassignable, non-escaping locals; anything
the analysis can't prove is rejected with a teaching error. Example: `75_arena_regions.jet`;
UI snapshots `tests/ui/arena_view_escape` (E0631), `tests/ui/arena_view_after_reset` (E0632);
unit tests `tests/arena.rs`.

## M6 phase 3 — multi-file imports (done)

Two use forms (S16): **quotes = file path, no quotes = module.**
**`use "path/to/file";`** — quoted path to a `.jet` file, relative to
the using file's directory (`use "./lib";` for a sibling file;
default namespace = last path segment). **`use name;`** or
**`use core.files;`** — unquoted module name (searches recursively from
the project root for `name.jet` or `name/{name,main}.jet`; `core` is a
compiler-exported module per S51). Optional **`as alias`** in both forms.

Cross-file access uses **`namespace.item`**; only **`pub`** items are visible from
other files (S18), including **`pub`** struct fields. A file may opt into
public-by-default with a single **`#PubFile`** marker (D-VISDEFAULT1=C /
D-VISDEFAULT2=A); inside such a file, top-level items export unless marked
**`priv`**. The driver loads the import
graph, sema checks the whole program, codegen emits one Rust file with **`mod`**
blocks and `user_<module>_<name>` mangling (`main` stays `main`).

Diagnostics: **E0602** path escapes the project · **E0603** missing import ·
**E0604** import cycle · **E0605** private item · **E0606** ambiguous module.
Example: `examples/features/modules/imports/` (three files; file import + `as alias`). UI
fixtures under `tests/ui/import_{escape,missing,cycle,private,private_field,ambiguous}/`.

**Library packages resolve through the same `use` (U17, D-LIB-USE A).** One
import concept covers files, modules, **and `library` packages**: once a
`library` package (U10) is realized — its source staged in the shared hangar
store by the `core` provider — `use <pkg>;` resolves to that staged tree and its
`pub` items are usable as `pkg.item` (S18). A realized library is simply found by
the same module resolver, with the hangar staging dir added as an extra search
root (the staged tree is searched exactly like the project tree or a path dep).
No new keyword, no `..` import, no special call form — it is an ordinary module
on the search path. An **`executable`** package goes on PATH, not `use`: naming
one in `use` is **E0982**. A package's **`kind` is inferred when omitted**
(D-ILE1): in a `pkg.jet` `packages:` block a bare `name` (no `: kind`), or a
package with no `pkg.jet` at all, resolves to `executable` when its source stages
a `bin/` or declares a top-level `fn run`, otherwise `library`; an explicit
`library`/`executable` always wins. Single-file `jet run`/`build file.jet` stays
executable-requiring (R9; E0101 if it has no `run`). A `library` dependency the project declares but hasn't
realized yet is **E0983** (run `jetpack build`) — `jet build`/`run` never realize
on demand, keeping them offline and deterministic, the same flow as pre-fetched
deps. Resolver: `Source/Loader.rs` (`collect_pkg_resolution`). Tests: `tests/lib_use.rs`
(offline realize → `use` → call) and `tests/ui/use_unrealized_library/`.

## Code module system (D-MOD1–4, done 2026-06-18)

Jet's module system is **Rust's, with two surface swaps**: the keyword is
`module` (not `mod`) and scoping uses `.` (not `::`). The `use "path" as alias`
form above stays as the ceremony-free single-file entry point.

**Declaration forms (D-MOD1).** `module math;` declares a file module — the
loader searches the using file's directory for `math.jet`, then `math/module.jet`;
neither found is **E0607**, both found is **E0606** (ambiguous). `module math
{ … }` is an **inline module** — its items live in the `math` namespace of the
containing file, no file lookup. `module` is shared with the JetOS declaration
(U3); the parser disambiguates by peeking past `{` (a code module body opens with
`fn`/`struct`/`pub`/… or `}`, a JetOS body with `sources`/`imports`/a
contribution path) and by the `;` form, which is always a code module.

**Access (D-MOD2).** Qualified `math.clamp(…)` always works. Optionally bring
items unqualified with `use math.clamp;` or a group `use math.{clamp, lerp};`.
Wildcards (`use math.*`) are rejected — **E0612**. Unqualified import of an
undefined item is **E0611**; of an item in a module not in scope, **E0610**.

**Visibility (D-MOD3, D-PUBPKG1).** Private by default; `pub` exports to every
consumer; `pub(package)` exports only inside the same payload/workspace package
boundary and stays hidden from downstream package consumers. A private item is
unreachable from outside its file or inline module: `math.helper()` where
`helper` is private is **E0609** (inline) / **E0605** (cross-file). An unknown
`pub(…)` qualifier is **E0411**. Inline-module function bodies are fully
type-checked, and a sibling call (`area` → `square`) lowers to the
module-mangled name (`geo__square`), so private siblings never leak into the
file's namespace or to rustc.

**Re-export (D-MOD4 — Rust-exact `pub use`).** A directory module's `module.jet`
exposes a submodule item only by re-exporting it: `pub use wrap.wrap;`. Nothing
auto-surfaces — a `pub`-but-not-re-exported item stays internal to the directory.
`text.wrap(…)` then resolves through the re-export to the defining module, with
the real function's borrow/move conventions preserved.

Examples: `examples/features/modules/inline_module`, `43_module_file`,
`44_module_dir`, `45_module_use_unqualified`, `46_module_use_group`,
`47_module_reexport`, `48_module_file_use`, `49_module_inline_sibling`,
`170_generic_modules`. UI
fixtures: `tests/ui/module_{missing,private,unknown_namespace,wildcard,
inline_private,inline_type_error}`, `genmod_{unknown_target,wrong_arg_count,non_fn_item}`.

### Generic modules (D-GENMOD1, D-GENMOD2, D-GENMOD-VALUE1,
D-GENMOD-BODY1, D-GENMOD-IDENTITY1)

A **generic module** is a module template parameterized by types and compile-time
values. Instantiating it produces a specialized ordinary module.

**Template form (D-GENMOD2=A):**

```jet
module Cache<K> {
    pub fn key_of(k: K) -> String { … }
}
```

Type parameters use PascalCase names with an optional bound (`K: Hash`).
Value parameters use lowercase names with a type annotation (`capacity: Int`).
Both live in one `<…>` list.

**Instantiation alias:**

```jet
module IntCache = Cache<Int>
```

Value parameters are immutable Tier-0 comptime `Bool`, `Int`, `Char`, `String`,
or fieldless-enum values (D-GENMOD-VALUE1=A). The compiler evaluates and
normalizes each value before specialization. `Int` parameters may appear in the
generic-module-only fixed-list layout form `[T#capacity]`. Value argument types
must match exactly; E0853 reports a mismatch.

The template body has ordinary-module parity and definition-site lexical scope
(D-GENMOD-BODY1=A). It may contain functions, structs, enums, tags, constants,
traits and impls, tests and benches, nested modules and generic modules,
aliases, and existing legal markers. Applying a template does not change what
its body can see or authorize.

Instantiation is applicative (D-GENMOD-IDENTITY1=A). The resolved template
DefinitionId and normalized arguments form the instance identity. Repeating
the same application shares nominal member types and one checked/code-generated
specialization; different arguments or a different template definition produce
a different instance.

**Implementation status:** the parser and AST represent templates and aliases,
and sema currently expands same-file aliases containing `fn` items. It
substitutes type parameters in function signatures. Value evaluation and body
substitution, bounds, cycles, full ordinary-module bodies, applicative identity,
cross-file templates, and the corresponding complete acceptance proof remain
open. E0854 is the current implementation boundary for non-`fn` items, not the
ratified language law. E0850 and E0851 are implemented. E0852 (unsatisfied
bound), E0853 (value type mismatch), E0855 (instantiation cycle), E0856
(disallowed value-parameter type), E0857 (argument is not a compile-time
value), and E0859 (identity fingerprint collision, ICE 101) remain staged until
their semantics, What/Why/Fix copy, and UI snapshots ship.

## M6 phase 4 — `--small` + LSP v0 (done)

**`jet build --small`** (S15): `opt-level=z`, fat LTO, `panic=abort`, stripped symbols.
Smaller binaries than the default speed-oriented profile (`tests/release_gates.rs` on
`examples/features/collections/wordcount.jet`).

**`jet self lsp`**: stdio JSON-RPC language server (hand-rolled JSON, invariant I6).
Capabilities: full-document diagnostics on open/change (real front end, including
import graph from disk with an in-memory overlay for the open buffer), S14
teaching-error quick-fixes (`Diagnostic.edit`), and formatting via `jet fmt`.
Scripted tests: `tests/lsp.rs`.

**VS Code / Cursor**: `editors/vscode/` — TextMate grammar + LSP client (plain
JS, no compile step; `install.sh` packs and installs the vsix). The client
auto-discovers the server: `jet.languageServerPath` setting, then
`<workspaceFolder>/target/debug/jet`, then `jet` on PATH. `jet self lsp` never
invokes rustc, so the cargo debug binary is sufficient.

## M8 — Functions as values (closures, done)

**Lambdas (S46):** `(params) => expr` or `(params) => { … }`. Parameter types
may be omitted when the expected function type is known (**E0801** when not).
The lambda arrow is **`=>`**; **`->`** stays for return types and
`if subject == { … }` dispatch arms.

**Function types (S47):** `fn(T1, T2) -> R` (no parameter names; `-> R` may be
omitted for no-return callbacks). Named `fn`s coerce to function values when
referenced without a call.

**Capture rules (S47):** shared read for names only read; mutable borrow for
names written (a `:=` binding required, else **E0111**). Escaping lambdas (stored in a
binding, returned, in a struct field, or passed to a `^T` parameter) must own
captures: clonable values are copied (**L0801**); non-clonable values need an
explicit prefix **`take(name)`** on the lambda (**E0802**). Self-recursion through
the binding is rejected (**E0804**). Calling a non-function → **E0803**.

**Collection methods:** `map`, `filter`, `each`, `find`, `any`, `all`,
`sort_by`, `reduce` on `[T]`; `each` on `[K: V]` (two parameters).

**D-ITER1 — lazy iterator adapter set (c105):** `take(n)`, `skip(n)`, `step_by(n)`,
`dedup()`, `chunks(n)`, `windows(n)`, `take_while(f)`, `skip_while(f)`, `flat_map(f)`,
`scan(init, f)`, `fold(init, f)`, `position(f)`, `min_by(f)`, `max_by(f)`, `group_by(f)`,
`partition(f)` on `[T]`. No new grammar — all are library methods on the iterator
protocol (D-EXT1 Tier 1). `take` is accepted in dot-method position even though `take`
is also the lambda-capture keyword. `enumerate()` and `zip(other)` return named tuples
`(idx: Int, item: T)` and `(a: T, b: U)` respectively; `partition(f)` returns
`(false_: [T], true_: [T])`. All lazy (evaluated at call site, allocation deferred to
result use).

D-S14-PAUSE: retired `lambda` / anonymous-fn spellings and `|x|` pipes get
ordinary parse errors. Current lambda syntax is `(x) => …`.

Examples: `examples/features/basics/closures.jet`, `examples/features/basics/callbacks.jet`,
`examples/features/collections/iter_adapters.jet`. Ui:
`tests/ui/lambda_*.jet` (E0801–E0804, E0204 mut-capture conflict,
E0507 collection change inside a `for` loop), `tests/ui/not_a_function.jet`,
`tests/ui/foreign_{lambda,pipe}.jet`; lint: `tests/ui_lint/lambda_escape_clone.jet`
(L0801). Integration: `tests/closures.rs`.

## M10 — Core library (done)

Full user-facing reference: **docs/reference/core-library.md**.

Compiler-known `core.<name>` namespaces backed by Rust std helpers in the
generated prelude (D-CORENS1/D-CORENS-CANON1): file/terminal/env/process I/O,
math, random, time, args, sized numeric types with checked-by-default
overflow, and unified `core.encoding` serialization (JSON/CSV/TOML/YAML over
one `DataTree` value, plus `@[Codable]` derive). Every fallible call returns
`T ? E`, handled with `?`/`??`/a pattern test like any M4 result. Importing a
module is free (R10) — codegen only emits the helpers a program actually
calls. See core-library.md for the full module list, signatures, and
examples; UI snapshots: `tests/ui/core_*`, teaching errors **E0037**–**E0039**.

D-CORE-COMPRESS1=A splits compression by job. `core.compress.gzip` and
`core.compress.zstd` are the only byte-stream codec homes; both expose
`compress` and fallible `decompress`. `core.archive` exposes zip/tar container
operations only (`zip_compress`, `zip_decompress`, `tar_add`, `tar_get`,
`tar_names_json`). It has no gzip re-export or compatibility alias.

## E2-M1 — Concurrency (tasks and channels, verified 2026-06-14)

`core.tasks` provides blocking tasks and typed channels. Import it as a normal
core module:

```jet
use core.tasks as tasks;
```

`tasks.spawn(() => work()) -> Task<T>` starts a task from a zero-parameter
lambda. The lambda must own every captured value: shared mutable captures are
**E1101**; use `take(name)` to hand a value to the task, or use a channel to
send results back. Values crossing the task boundary must be sendable
(**E1102**): no `view` borrows, no structs that contain `ref` fields, no trait
values, and no closures unless handed over with `take`.

`task.join() -> T` waits for the task and consumes the `Task<T>` handle. Calling
`.join()` twice is ordinary use-after-move (**E0121**). Dropping a `Task`
without joining or detaching emits **L1101** because the program may end before
the task finishes. A panic inside a task is reported when joined and exits with
the runtime panic code.

`task.detach()` (D-DETACH1) fire-and-forgets the task — it consumes the
`Task<T>` handle so **L1101** is suppressed, and the task continues running in
the background. Detach is sound only when the spawned lambda holds fully-owned
data. Two detach-site diagnostics guard unsound cases:

- **E1106**: the lambda returned or captured a `view` borrow — a detached task
  may outlive the borrow's source, so the `view` would dangle. Fix: pass an
  owned `copy` or `share` instead.
- **E1103**: the lambda had a different sendability failure at spawn (E1102
  already fired); detaching an unsound task is doubly dangerous.

D-COROUTINE1 keeps coroutine machinery internal and exposes expert control via
task handles instead of new `coroutine` syntax. `task.wait()` aliases
`task.join()`. `task.pause()`, `task.resume()`, and `task.cancel()` set
control-plane state on the handle; `task.trace() -> String` reports
`paused=...,cancel=...`. Pause holds a running task at its next wait point until
`resume()`; these are enforced by the M:N scheduler, not mere flags.

D-CANCELMODEL1=C (ratified 2026-07-11): cancellation is **preemptive at wait
points**. A cancelled task — a race loser, a fail-fast sibling, or an explicit
`handle.cancel()` — unwinds at its next wait point (channel receive/send,
`time.sleep`, task join, a `select` arm, I/O), running Drop-backed (RAII)
cleanup on the way out, exactly as a blown deadline (E3003) already does. There
is one unwind mechanism with two triggers (deadline, cancel). A cancelled task
does not fall through to the code after the wait: a cancelled `receive()` unwinds
instead of returning `Closed`, and a race loser stops at its next wait point and
releases resources via Drop rather than running to completion. A cancelled
`g.all` member reports `Cancelled` rather than a completed value. A scoped
shielded region defers (never discards) the unwind until a critical section
finishes — its wait points complete normally and the deferred cancel/deadline
lands when the region exits. D-SHIELDNAME1=A spells that lexical region
`#Shield { … }`. It takes no arguments, nests by depth, and always leaves through
an RAII guard, so return, error propagation, and unwinding cannot strand a task
in the shielded state. At the outermost normal exit, an expired deadline lands
before a pending cancellation. Outside a task, the region is a transparent
block; at comptime it has no scheduler effect.

`tasks.channel<T>() -> (Sender<T>, Receiver<T>)` (D-TUPLE-DESTRUCT1) creates a
linked send/receive pair, destructured at the call site: `(tx, rx) :=
tasks.channel<T>()`. A second sender is `copy tx` — there's no combined
"channel" value to fetch one off of. `sender.send(value)` moves a `T` into the
channel (ownership semantics for non-copy values), and
`receiver.receive() -> T ? Closed` blocks until a value arrives or all senders
are gone. Channel payloads
must be sendable (**E1102**).

D-DEADLINE1 (ratified 2026-06-28): an ambient deadline can be set with
`#Context(deadline: <Int epoch_ms>) { … }`. Inside that scope, wait/IO points
observe the inherited budget (task joins, channel receive, `time.sleep`, TCP
read/write stubs). When the budget is exceeded, runtime report **E3003** is
emitted in Jet terms and execution exits with the runtime error code.

Teaching errors: **E0040** points `async`/`await` users at `tasks.spawn`;
**E0041** points `Mutex`/`lock` users at channels.

### Taskgroups and structured combinators (D-TASKSCOPE1, D-CONCCOMB1, D-RACEWIN1, D-CONCSELECT1; verified 2026-06-30)

Structured concurrency uses a scoped `taskgroup` (D-TASKSCOPE1=A). Inside
`taskgroup g { … }`, `g.task { … } -> Task<T>` spawns a child owned by the
group. Unjoined handles at scope exit are cancelled and joined before the block
returns.

Combinators are methods on the group handle only (no detached work):

- `g.all([t1, t2, …]) -> [Task]` — every task must succeed; fail-fast cancels
  siblings and exits with `panic: a task panicked` (example `169_all_failfast.jet`).
- `g.race([t1, t2, …]) -> T` — first **successful** result wins; losers are
  cancelled (D-RACEWIN1; example `167_race_cancel.jet`).
- `g.any([t1, t2, …]) -> T` — first **completion** wins, including errors.
- `g.select()` — fluent scoped multiplex (D-CONCSELECT1=A):

```jet
winner :: g.select().recv(ch1).recv(ch2).after(ms).wait()?
```

`.recv(receiver)` registers a receive arm; `.after(ms)` a timer arm; `.read(stream)`
is reserved for stream I/O (stub until networking lands). `.wait()` blocks until
one arm wins, deregisters losers, and returns the received value. Example:
`168_select_channel.jet`.

The M:N scheduler (D-ASYNCRT1=A) parks tasks at channel/timer/IO waits instead
of blocking OS threads. Native I/O pollers: Linux `epoll`, macOS/BSD `kqueue`,
Windows IOCP path falls back to portable poll with an honest metric until IOCP
lands. Task-local Jet traps unwind into the scheduler so sibling combinators can
report `panic: a task panicked` instead of exiting the whole process early.

Scale tests: `scheduler_spawn_10000_tasks` in CI; `scheduler_spawn_100000_tasks_bench`
is `#[ignore]` for local 100k stress.

## Modules — `module name { … }` (U3, unified-ecosystem §4–5; parser, Stage 1a)

A module is a named, composable top-level declaration that contributes typed
values to reserved namespaces. Many modules may share a file.

```ebnf
module      = "module" dashed-name "{" contribution* "}" ;
contribution = namespace "." dashed-name ":" expr [","] ;
namespace   = "env" | "image" ;
dashed-name = ident { "-" ident } ;                (* S84: kebab-case names *)
```

- **Dashed names (S84):** package / module / image / env **names** may
  be kebab-case — `module web-app`, `env.web-tools`, `image.halcyon-oci` —
  matching nixpkgs/npm convention. A `-` joins two segments only when it is
  *span-adjacent* to both (no surrounding whitespace), so a spaced `a - b` stays
  subtraction; this is a parser rule (`expect_dashed_name`), not a lexer or
  expression-grammar change. Code identifiers (variables, fields, types,
  functions) stay plain `ident`. No leading, trailing, or doubled hyphen.
- **Disable with a leading underscore:** `module _name { … }` parses with
  `disabled = true` (the name begins with `_`); it is not discovered or merged
  (U3, one-character reversible toggle).
- **Active reserved namespaces** are `env` → `Env` (dev environment),
  `system` → `System` (jetos host), and `image` → `Image` (OCI container image
  or jetos installer input).
- **`env.<name>:` values reuse the ordinary expression parser** — typically a
  struct literal (`Env.{ packages: […], prompt: "…" }`), so lists and strings
  work with no new grammar. `prompt: "name"` is the beginner shorthand. For
  prompt depth, write `prompt: Prompt.{ label: "name", path: .Short, strip: .On }`
  (`path: .Full` and `strip: .Off` are the other modes). `jet env` renders that
  as one hybrid prompt: label plus path by default, `Ctrl-G` status glance on
  demand, and the optional strip showing the same status words.
- **`image.<name>:` values are Jetpack OCI images.** Active fields are
  `kind: .Oci` (optional when `from: packages.<name>` makes it clear),
  `from: packages.<name>`, `expose: [Int]`, `env_vars: ["KEY": "value"]`,
  `files: [String]`, and `base: oci("<ref>")`. `base:` is captured but not yet
  realized because registry-pull is gated on TLS/native-client work. `.Iso`,
  `.Qcow`, `.Raw`, and `from: system.<name>` are jetos installer inputs handled
  by `jet os image`, not by `jet image`.
- **Ad-hoc adapters (U20):** an `env.<name>.packages` list may contain
  `Pkg.adapt(name:, source:, recipe:)`. `source:` is a provider ref such as
  `path@vendor/tool`; this U20 slice realizes `Recipe.copy()` and
  `Recipe.prebuilt(bin:, as:)` into ordinary hangar packages, with the same
  store/lock path as any other package. `jetpack add <ref> --adapt` prints a
  draft adapter and does not run upstream code.
- **No-Nix machines (U23):** core packages and adapted packages realize without
  Nix. Package refs that still route through the Nix compatibility provider are
  reported together as E1272, naming only those holes and suggesting either
  installing Nix or drafting an adapter with `jetpack add <ref> --adapt`.
  Foreign-flake commands remain E1256 because they cannot run at all without
  the `nix` binary.
- **Offline discovery (U26):** `jet search <query>` and
  `jet info <source>.<package>` read only `.jet/discovery/index.jsonl`, local
  provider fixtures, and hangar metadata. They never fetch. `--json` emits the
  same package records the editor discovery hooks consume: source, name,
  resolved ref, version, platforms, docs, provenance, and typed service option
  fields.
- **Failed-build debugging (U27):** recipe-backed builds persist per-step logs
  under the hangar. On failure, scratch is preserved in hangar-managed storage
  and the diagnostic names `jet logs <pkg>` plus `--shell-on-fail`. Package-form
  `jet explain <ref>` prints the latest resolution/build record; code-form
  `jet explain E1234` keeps the existing diagnostic essay behavior.
- **Hybrid CLI output (D-FE-CLI1):** trivial reads stay quiet, multi-package
  realization/build work reports dependency-chain progress, and mutations plan
  before applying. Plan rows use `+`, `-`, and `~` in both colored and plain
  output. `-y` and `--yes` are the same confirmation bypass; non-interactive
  mutation without either prints the plan and changes nothing. Diagnostic text
  and JSON output remain unchanged.
- **Package overlays and overrides (D-JPK-OVERLAY1):** `workspace.jet` may carry
  reviewed overlay policy inside `module workspace`. An `overlay <name> { ... }`
  block records provider/channel swaps (`provider: Provider.nixpkgs(channel:
  "plasma-beta")`), per-package source/version/flag/patch changes
  (`package("foo").patches += [patch("patches/foo.patch")]`), and package-local
  `allowUnfree`. Workspace-wide unfree review uses
  `policy.allowUnfree: ["discord"]`. `jetpack override draft <ref> --patch
  <file>` only writes that source policy; it never creates hidden state. Patch
  application is deterministic unified-diff application against the source tree,
  and `jetpack explain package-overlay:<overlay>:<package>` prints provider,
  channel, policy fingerprint, and update command from the same source policy.
- **No daemon / no root (U28):** jetpack is a one-shot, user-owned process:
  no resident daemon, no root-owned default hangar, no privileged sandbox
  helper. Concurrent commands coordinate through file locks. If unprivileged
  sandboxing is unavailable, Jetpack emits L0205; `jetpack config sandbox
  require` makes that condition E1275 instead.
- **Universal trust grants (D-JPK-GRANTCMD1/SCHEMA1):** `jet trust` is the
  public command family for the unified grant graph. `jet trust list` shows
  package, build, env, service, image, fleet, and jetos authority grants;
  `jet trust explain [<grant>]` expands exact authority and revocation keys;
  `jet trust grant <grant> [--scope user|repo]` records a reviewed local grant;
  `jet trust revoke <grant>` removes it so the next risky action asks again.
  The store remains backward-compatible with U19 `hash:` and `pattern:` lines.
  `pkg.jet` may carry reviewed source policy as
  `policy: { trust: { default: prompt, ci: { prompt: deny }, services: { postgres: prompt } } }`.
  Policy decisions are `allow`, `prompt`, or `deny`; unknown fields are a
  manifest error.
- **Offline guarantee (U29):** once a package ref is realized into the hangar,
  realize-class verbs can use it again with `--offline` and no provider
  metadata refresh. Network-class verbs (`add`, `update`, `outdated`,
  publish/cache sync) refuse `--offline`, and a missing local object reports
  E1276 instead of fetching or timing out.
Stage 1a is parser-only for the AST shape; the jetpack module evaluator
(`Source/Jetpack/ModuleEval.rs`) gives these contributions meaning (field-checking +
capture into a plan model). The U5 merge engine consumes `env` contributions.

### Jetpack Services And Secrets

- **Dev services (D-JPK-SERVICE1):** an `env.<name>` role-module may carry
  `services: { name: { enable: Bool, ... } }`. These are project-local
  processes managed by Jetpack for the dev loop (`jetpack services
  up/down/health/logs`). They are not system services and do not imply jetos
  activation.
- **Secrets (D-JPK-SECRETCRYPTO1):** an `env.<name>` role-module may
  declare `secrets: ["name", …]` — a plain `[String]` list, no dedicated
  grammar. Each name is one this env expects to find in the project's
  encrypted repo store (`.jet/secrets.age`, managed by `jetpack secrets
  set/get/recipients/keygen`); reading one at runtime is `core.vault.get`,
  gated by the `Secret` effect (**E1264** if ungranted) and unconditionally
  denied at build/comptime time (**E1265**, no `#Impure` escape hatch).
  `jetpack secrets get <name>` on a name absent from the store is **E1263**.

### jetos Runtime Slice

`jet os check|init|plan|proof|build|switch|rollback|generations|lift|import|image|vm`
is active. A bare host (`jet os switch laptop`) selects `system.laptop` in
`./config.jet`; `path@host` selects an exact external root. Builds create named
generations; `generations` lists newest first; `switch --name <name>` overrides
the automatic name; `rollback` activates a prior generation. `plan` prints the
checked system plan without building. `proof` reads the latest generation's
plan, proof, provenance, health, boot, init, secrets, VM, and rollback facts.
The current slice records a root `sw/bin` package closure, hangar/cache facts,
systemd service/timer/socket units plus target wants, users/groups, filesystems and swap,
networkd/firewall/wireless facts, Limine + CachyOS boot facts, first-party
systemd init closure with `/sbin/init`, kernel firmware/driver facts, desktop/display-manager facts,
terminal login facts with serial/virtual getty units and user home/profile
projection,
NixOS/flake-parts/Home Manager import through `jet os import <flake-or-dir>`
with semantic `jetos-import-facts.json` input and audited facts-only fallback,
per-user generation profiles under `users/`, Flatpak exact reconcile plans,
permission overrides, undeclared-app removal, and AppImage runtime integration under
`flatpak/`/`appimage/`, performance profile, sysctl, zram, sched-ext scheduler, initrd, and
bootloader tuning proof under `performance/`, option priority
explain output under `module-system/`, storage plans, fstab projections,
safe-by-default `jetos-storage-apply`, and `jetos-persist-activate`
impermanence proof under `storage/`, container/microVM workload plans with
mounts, secrets, resources, health, and rollback proof under `workloads/`, hardware scan source,
profile manifests, boot specialisation entries, and `jetos-hardware-scan` /
`jetos-hardware-doctor` commands under `hardware/`, reusable theme projections under `theme/` and concrete GTK,
Qt, terminal, editor, display-manager, and Studio preview files, and
`jetos-service-logs` journal/fallback log query support under
`service-manager/`,
fleet deploy plans plus generated `jetos-fleet-deploy` host scripts under
`fleet/`, runnable workload launchers under `workloads/`, a generated
`jetos-flatpak-reconcile` command for remotes/apps/permissions/removals,
`jetos-appimage-run` for AppImage desktop integration, lifecycle channel policy,
proof-gated auto-upgrade service/timer, rollback-on-health-fail proof, and
explainable `jetos-lifecycle-gc` retention scripts under
`lifecycle/`, option priority/explain output under `module-system/` with
winner/loser contenders and disabled-module manifests, typed option
reference/search artifacts under `options/` including type, default, example,
doc, tier, priority, and provenance plus exact/explain search modes, and image
variant artifacts under `systems/images/`: qcow2, raw, SD-card image, and a
netboot bundle with kernel/initrd/iPXE config plus
`jetos-image-variants-<host>.proof.json`,
first-wave `apps.program.*` modules under `apps/programs/` for git, ssh, fish,
starship, ghostty, helix, yazi, btop, bat, eza, fzf, zoxide, ripgrep, tealdeer,
fastfetch, VS Code, Cursor, Discord, Spicetify, and browser policy projection,
plus `jetos-app-module-apply`,
desktop breadth projections for PipeWire/rtkit, GNOME and Plasma Wayland
sessions, libvirt/SPICE/swtpm/USB redirection, GameMode/Steam/Proton policy,
locales/keymaps, XDG mime defaults, fonts, smartcard pcscd, and AppImage binfmt,
owner-`~/nixos` acceptance coverage matrix, VM gate list, no-omission diff, and
`jetos-acceptance-prove` proof under `acceptance/`,
repo-ciphertext/host-key tmpfs-only secret activation proof, guided-ext4 disk
intent with `--manual`, and `jetos` hybrid ISO media staging/proof. When pinned
xorriso, Limine, zstd, QEMU, and filesystem tools are present, `jet os image`
writes the ISO artifact, the qcow2/SD/netboot variant proof, and records the
exact tool paths in proof JSON.
Generated terminal profiles set `JETOS_BRAND=JetOS` and a clean `JetOS <host>`
prompt for login shells and VM run-mode shells; `/etc/issue` and `/etc/motd`
carry the same light host branding.
Fleet deploy scripts default to tar-over-SSH staging, remote proof before
switch, health check, and rollback-on-fail; tests may replace those commands
with local hooks, but the generated default is a real push path, not a proof
label. Lifecycle GC is explain-before-delete by default and deletes old
generation directories only when invoked with `--apply`.
Installer media copies the generation as a self-contained tree; host-root
symlinks are dereferenced before the ISO is built so the guest install does not
depend on the build machine's paths.
Compatibility escape
hatches such as overlays and specialArgs are allowed only as explicit
`packages.*` options; each one is written to the generation's compat audit file
and provenance so Studio can show it and native replacement work can track it.
`jet os vm prove <host> --disk <path>` is the install/reboot proof entrypoint;
`--real` upgrades it to replacement acceptance and rejects script/fake VM tools.
it fails with E1279 rather than faking boot media when pinned QEMU/media tools
are missing, and it fails with E1285 rather than treating a prepared QEMU harness
as a passed guest proof. It writes the VM proof harness, runs the recorded QEMU
create/install/reboot phases, captures a `JETOS_GUEST_PROOF:` marker from the
installed guest's serial output, and writes the guest proof artifact. The QEMU
installer phase boots the hybrid ISO with `console=ttyS0`; the ISO's Limine
entry carries `rdinit=/jetos/init`, `jetos.mode=install`, and the target
disk. The installer writes a GPT disk with a FAT ESP, ext4 `jetos-root`,
`EFI/BOOT/BOOTX64.EFI`, the kernel/initrd, and an installed Limine config. The
installed-disk verifier phase boots that disk through firmware/Limine with
`rdinit=/jetos/init`, `jetos.mode=verify`, and the installed root label. A rerun
may promote the harness to `guest-passed` only when the guest proof records the same
host, generation, disk, media proof, tool hashes, and required guest assertions;
harness JSON names the expected guest proof path, command argv, and per-phase
run logs. The proof then boots a graphical desktop verifier with QEMU VNC,
stdvga, and the packaged `bochs` DRM module; promotion requires the guest to see
a framebuffer (`fb0`) and execute the generated display-manager,
desktop-session, and terminal-fallback launchers in proof mode before it emits
`graphical-console-ready` and `desktop-launchers-run`. The required guest
assertion set includes terminal-login readiness, desktop-session readiness,
graphical-console readiness, and launcher readiness:
the installed generation must carry `terminal/facts.json`, `/etc/profile`,
`/etc/shells`, enabled `serial-getty@ttyS0.service`, and the user home profile
inside the root projection, plus `desktop/facts.json`, the GNOME Wayland session
launcher, the display-manager unit, the terminal fallback launcher, the installed
jetos Studio app, a guest-visible graphical framebuffer, and launchers that
resolve GNOME/GDM commands from the installed system closure before VM proof can
pass. The
ratified interactive launch surface is `jet os vm run <host> --disk <path>`;
it opens only a disk already tied to the latest generation by a `guest-passed`
VM proof, exposes a graphical VNC console, and attaches serial output to the
current process. It fails with E1287 rather than launching an unproven qcow2. The
default CachyOS kernel is a first-party `cachyos-kernel`
source-built package carrying recipe/config/patch/initrd-input hashes beside the
kernel and initrd artifacts; missing kernel package provenance is E1280, missing
bootable kernel/initrd artifact headers are E1282, missing source recipe
or builder provenance is E1284, and a failing `source/build.sh` bootstrap build
is E1286.
The build script is package-internal and authoritative: when the source recipe
is present, `source/build.sh` runs before boot validation even if stale boot
files already exist. `JETOS_KERNEL_SOURCE`, `JETOS_KERNEL_OUT`, and
`JETOS_KERNEL_PACKAGE` point at the realized first-party package, and the script
must write the kernel and initrd artifacts that the generation, installer, and
VM proof will boot. Installer media appends a JetOS initrd overlay containing
`/jetos/init`, `/jetos/install.sh`, and `/jetos/guest-verify.sh`; `/jetos/init` dispatches
`jetos.mode=install`, `jetos.mode=verify`, and `jetos.mode=desktop-verify`,
mounts proc/dev/sys before reading cmdline, probes `LABEL=jetos-root`,
`/dev/vda2`, and `/dev/sda2` before falling back to install mode, and the ISO
Limine config enters this dispatcher. The hybrid ISO
carries both BIOS Limine boot files and a FAT `boot/efiboot.img` ESP with
`EFI/BOOT/BOOTX64.EFI`, so QEMU/OVMF and physical UEFI firmware boot the same
installer artifact. The graphical verifier phase still direct-boots the same
kernel/initrd with `rdinit=/jetos/init` so it can force a VNC/stdvga display for
desktop proof; the installed-disk verifier uses firmware disk boot. The verifier
emits the serial guest-proof marker. The default systemd
init path requires a first-party `systemd` package; missing init provenance is
E1281. Each generation defaults to the ratified GNOME-on-Wayland desktop profile
with terminal login as the secondary fallback. The display-manager service
launches the system session when GNOME/GDM are present and falls back to the
terminal launcher instead of blocking boot. Each generation also installs the
first-party jetos Studio app projection under `sw/bin/jetos-studio`,
`share/applications`, and `studio/`;
`studio/data.json` carries the read-only host/package/service/option projection
artifact paths, no-plaintext secret policy, adaptive fleet surface, separate-app
Canvas deep-link metadata, and changeset apply gates. The root projection carries these files into
`/run/current-system`. Studio remains a separate jetos system app from Canvas
and may fall back to the browser over the same local projection service. It may
deep-link to Canvas for generic source graph editing, but it stores no Canvas
semantic state. The
direct launch command is `jetos studio`; `jetos studio --headless` prints the
installed app path for CI/review without opening a browser, and `--json` prints
the root, app, metadata, and data paths without opening a browser.
`jetos studio --serve <loopback:port>` serves `index.html`, `app.json`, and
`data.json` over the local browser fallback. `GET /studio/source` serves the
selected `config.jet` for the adjacent source pane. The same local service
accepts source transactions at `POST /studio/transaction`; the first implemented
transaction is `set-option`, which returns an exact source diff and writes
`config.jet` only when `write:true`. `POST /studio/run` executes the matching
`jet os check|plan|build|proof|generations` action from the selected
project/host and returns captured output, so Studio never substitutes hidden
state for CLI proof. The build action writes a named Studio candidate generation
before proof.

`module vmtest.<name>` declares a JetOS VM scenario. Its `hosts:` map names
scenario handles bound to `system.<host>` declarations, and `run: test { ... }`
captures typed host-handle assertions such as `wait_for_boot`,
`assert_unit_active`, and `assert_port_open`. `jet os vm test <name> --disk
<path>` evaluates that scenario through the same installer/reboot proof harness
as `jet os vm prove`, writes one host proof per scenario host, then records
`systems/vm-tests/<name>-vmtest-proof.json` with the source test body,
assertion method facts, host generations, disks, and proof artifact paths.

`jetos user plan|build|switch|rollback|prove <name>` is the standalone
per-user path. It selects a `user.<name>` or `users.<name>` profile from
`config.jet`, renders the same `users/<name>/profile.json` artifact used by
`jet os switch`, and builds/activates/proves it through normal named
generations rather than a separate hidden state store. The generated
`jetos-user-apply <name>` command applies that profile to a home directory:
projects declared files, links package binaries into `.jetos/profile/bin`,
writes user service units under `.config/systemd/user`, and records
`.jetos/proof/user-<name>.json`.

## Fan-out operator `f.[a, b, c]` (S75) and fixed-size list `[T#N]` (S76)

### Fan-out `f.[a, b, c]`

`f.[a, b, c]` is syntactic sugar that expands to `[f(a), f(b), f(c)]`. The
callee `f` must be a one-argument function; each item is type-checked against
`f`'s parameter type. The result type is `[R#N]` where `R` is `f`'s return
type and `N` is the number of items.

```ebnf
fan_out = expr ".[" [ expr { "," expr } [ "," ] ] "]" ;
```

```jet
fn double(n: Int) -> Int { return n * 2; }

doubled :: double.[1, 2, 3];  // : [Int#3]  →  [2, 4, 6]
```

Errors: **E0961** if the callee is not a one-argument function; **E0962** if an
item's type doesn't match the parameter type.

### Fixed-size list `[T#N]`

`[T#N]` is a type refinement meaning "a list of exactly N elements of type T."
It is produced by fan-out and can be destructured with an exact-count pattern.
At codegen it erases to `Vec<T>` (same as plain `[T]`).

```ebnf
type_fixed_list = "[" type "#" int_literal "]" ;
```

```jet
result@ [Int#3]=  double.[1, 2, 3];
[a, b, c] :: result;   // OK — 3 names for 3 elements
```

- Destructuring a `[T#N]` with the wrong number of names is **E0963**.
- Calling `push`, `pop`, `insert`, `remove`, or `clear` on a `[T#N]` is **E0964**.
- A literal index outside `0..N-1` on a `[T#N]` is **E0965** (compile-time check).
- A `distinct Int` with `#Invariant("value >= lo && value < hi")` may index a
  `[T#N]` without a runtime bounds check when `lo >= 0` and `hi < N`.
- `[T#N]` is accepted wherever `[T]` is expected (widening coercion); the
  length information is erased at that point.

## Effect system (D-EFF1, D-QUAL1, D-EFF2, D-EFF3)

Every function carries an **effect set**: the categories of ambient power its
body exercises — touching the network, the filesystem, the clock, and so on.
The set is **inferred**, never declared by default, **propagated along calls**
(a caller's set includes every callee's set), and **fully erased in codegen**
(I3) — effects are a compile-time proof, with no runtime value, handler, or
monad. A `@Pure fn` is exactly the function whose inferred set is empty.

### The effect vocabulary

Effects are a closed, compiler-known set of PascalCase tags (D-CASING1). Each
primitive Core operation contributes one effect; an effect appears in a
function's set when the function reaches an operation that carries it.

| Effect  | Carried by |
|---------|-----------|
| `Io`    | `print`, `eprint`, `input`, `read_all_input`, `core.io.*` |
| `Fs`    | `core.files.*` (whole-file helpers and streaming handles), `core.watcher.files` |
| `Net`   | `core.net.*`, `core.http.*`, `core.watcher.port` |
| `Time`  | ambient `core.time` clock/zone reads (`now`, `now_utc`, `today`, `instant`, `zone`, `sleep`, `start`) |
| `Rand`  | `core.random.*` |
| `Env`   | `core.env.*` |
| `Exec`  | `core.process.run`/`exit`/`cmd`/`pipeline`, `ProcessSpec.run`/`spawn`, `ProcessChild` wait/control/stream calls, `core.watcher.process_pid` |
| `Db`    | `core.db.*` |
| `Log`   | `core.log.*` |
| `Gpu`   | `core.raylib.*`, future `core.gpu.*` / `core.game.*` |

A call to an `extern rust`/C foreign function, whose body the compiler can't
inspect, contributes the **maximal** set (every effect) — it is assumed to do
anything. This keeps inference sound without reading foreign code.

### HTTPS client default (D-TLS1)

`core.net.fetch` and `core.http.client` support `https://` in the default build
through the rustls bridge and system certificate roots. Plain `http://` remains
available for loopback fixtures and old endpoints. HTTPS client failures are
reported in Jet terms: E4201 for handshake failure, E4202 for certificate trust
failure, and E4203 when the host image has no usable certificate roots.
Advanced client TLS configuration belongs in `core.tls` (custom roots,
pinning, client certificates). D-TLSSERVE1=A adds HTTPS serving as a named
option on the same server entry point: `Server.serve(addr, mux, tls:
Server.tls(cert, key))?`. The third argument must be labeled `tls:`; unlabeled
TLS config is rejected so the transport switch is visible at the call site.

### Graphics and games (D-RAYLIB1, D-GAME1-3)

`core.raylib` is the first-party graphics bridge package. The typed surface is
`window_open`, `window_should_close`, `window_ready`, `begin_drawing`,
`clear_background`, `draw_rectangle`, `draw_text`, `end_drawing`,
`close_window`, `key_down`, `set_target_fps`, and `color`. By default the
bridge runs headless so CI does not need a display server. With
`JET_RAYLIB_DISPLAY=1`, generated code
dynamically loads the native raylib shared library and calls the real C API; if
the library is absent, it degrades to the same headless path.

`core.game` is the flagship engine name (D-GAME2=A). Its public beginner API is
scene-first with a frame hook (D-GAME3=C): a `Scene` owns durable editable game
data, while `scene.on_frame((frame) => { ... })` attaches per-frame logic.
The current Core floor is headless and deterministic: `game.Scene.new`,
`scene.assets.image`/`sound`, `scene.input.bind`, `scene.component<T>()`,
`scene.query<T...>()`, `scene.budgets.set(game.Budgets.new(...))`,
`game.Replay.record`, `game.Backend.headless`, and
`game.run(scene, replay: replay)` produce a stable transcript without renderer,
audio, editor, or file-backend dependencies.

### Declaring a boundary — `#(…)` on the signature

A function may pin an **upper bound** on its effects by writing `#(E1, E2, …)`
on its signature, between the parameter list and the return arrow:

```ebnf
fn_effects = "fn" ident "(" params ")" [ "#(" [ effect { "," effect } ] ")" ]
             [ "->" type ] block ;
```

```jet
fn load(path: String) #(Fs) -> String {
    return core.files.read(path)?;     // OK: Fs ⊆ {Fs}
}
```

The compiler infers the body's real effect set and checks it is a **subset** of
the declared bound. An effect the body uses that the bound omits is **E0740**,
naming the effect, the call that introduced it, and the declared set. `#(…)` is
an assertion the author makes a contract — the inferred set may be *smaller*
than the bound (the bound is a ceiling, not an exact set), but never larger.

`@Pure fn` is the same contract with an empty bound: any effect at all is a
purity violation (reported as **E3401**, the established purity diagnostic).
Writing `@Pure fn f() #(Fs)` — a non-empty bound on a `@Pure` function — is a
contradiction, **E0745**.

Effects are erased: `#(Fs)`, `@Pure`, and an unannotated function with the same
body all generate byte-identical Rust.

### Restricting a region — `#Caps(…) { … }`

Where `#(…)` bounds a whole function, `#Caps(…) { … }` restricts a **block**.
Inside the region, the only effects allowed — directly or through any call it
reaches — are the ones listed; anything else is **E0741**. It is a hard local
ceiling, not a grant: the effects still happen and still count toward the
enclosing function's set.

```ebnf
caps_region = "#Caps" "(" [ effect { "," effect } ] ")" block ;
```

```jet
fn run() {
    #Caps(Fs, Io) {
        text :: core.files.read("x") ?? "";   // Fs — allowed
        print(text);                            // Io — allowed
    }
}
```

A call inside the region that transitively touches `Net` would be E0741 even
though no `Net` call appears literally in the block. Like every effect
construct, `#Caps` is a plain lexical block in codegen — it erases.

### Higher-order effects — transparent flow-through (D-EFF2)

A higher-order function's effect set is **its own body plus, at each call, the
effects of the function values passed to it** — so a callback's effects surface
at the *call site*, not buried inside the higher-order callee. This is the
zero-syntax default:

```jet
fn apply(f: fn(Int) -> Int, x: Int) -> Int { return f(x); }

fn run() #(Io) {
    apply(log_it, 1);   // if `log_it` uses Net, this line is E0740 — Net ⊄ {Io}
}
```

- A **lambda** argument's body is walked inline, so its effects already belong
  to the enclosing function.
- A **directly-named function** argument flows its effects through precisely.
- Any **other** function value (a local binding, a parameter passed onward, a
  returned or stored callback) has an origin that isn't statically known at the
  call, so it defaults to the **maximal** effect set — sound, conservative.

Two expert levers refine this (ratified D-EFF2, additive to the default above):
`@Pure fn(…)` / `#(Net) fn(…)` **parameter types** demand/bound a callback
(passing one with effects outside the bound is **E0744**), and `#(via f)` on a
signature publishes a tight pass-through that holds even when the value escapes.
The conservative default is correct without them; they trade syntax for
precision.

### Effects on trait methods (D-EFF3)

A trait method may declare an effect upper bound — `@Pure fn hash(self)` (the
empty set) or `fn render(self) #(Gpu)`. The bound is two things at once:

- **The impl obligation.** Every implementation's inferred effects must fit
  inside the bound, or it is **E0742**. So a trait can promise "all `hash`
  implementations are pure" and the compiler holds every impl to it.
- **The dispatch contract.** A call through a trait object (`Box<dyn Trait>`)
  sees the declared bound as its effect, because the concrete impl is unknown at
  the call site — so safe-by-default survives dynamic dispatch.

```jet
trait Shape {
    @Pure fn area(self) -> Int;   // every impl must be pure
}
impl Square.Shape {
    fn area(self) -> Int { return self.side * self.side; }   // OK — pure
}
```

An un-annotated trait method is inferred per-impl under static dispatch; the
dynamic-dispatch fix-it (annotate the method when it's called through an object
under an effect ceiling, E0743) is the remaining surface here.

## Terminal direct-input (D-TERM1, ratified 2026-06-22)

`live { … }` enters un-buffered, no-echo terminal input mode for its body and
restores the terminal on every exit path (normal return, `?` propagation, panic
unwind) via a RAII scope guard (D-DEFER1).

```jet
use core.term as term

live {
    k :: term.read_key()
    if k == Enter { return }
    print("got: {k}")
}
```

`use core.term as term` is required for `term.read_key() -> Key`. The `live`
keyword itself does not require the import — the block's syntactic gate is
sufficient.

**`Key` enum** (prelude type, `core.term`):

| Variant | Payload | Description |
|---------|---------|-------------|
| `Char(c)` | `Char` | Printable character |
| `Enter` | — | Enter / Return |
| `Escape` | — | Escape |
| `Backspace` | — | Backspace |
| `Tab` | — | Tab |
| `Delete` | — | Forward delete |
| `Up` / `Down` / `Left` / `Right` | — | Arrow keys |
| `F(n)` | `Int` | Function key F1–F12 |
| `Ctrl(c)` | `Char` | Ctrl + character |
| `Unknown` | — | Unrecognised byte sequence |

Pattern matching uses `== Variant(binding)` (PatternTest form):

```jet
if k == Char(c) { print("char: {c}") }
if k == Enter   { break }
if k == F(n)    { print("F{n}") }
```

Enum literals use the qualified form: `Key.Char('a')`, `Key.Enter`, etc.

**Restrictions:**
- E3401: `live { … }` is impure — rejected in a `@Pure fn`.
- E3301: rejected in `--freestanding` builds (no OS terminal device).
- REPL: rejected in interactive mode.

**Platform FFI:** I6-compliant; uses inline `extern "C"` (POSIX termios) and
`extern "system"` (Windows console API) — no external crates.

## REPL Core effects

The REPL keeps accepted statement ASTs and live `CtValue`s across turns.
Lists, maps, options, results, structs, enums, and closures are not rebuilt
from display text; explicit binding annotations remain available to `:type`.

Pure Core calls run directly. Ambient Core calls use normal Jet authority:
the call must be inside `#Grant(root)`, and the REPL must authorize the exact
operation and resource before it touches host state. A TTY prompts for once,
session, or deny. A session allowance is an exact tuple and offers continue
or revoke on reuse. `--allow-fs`, `--allow-env`, `--allow-exec`,
`--allow-net`, and `--allow-io` skip ordinary prompts for their roots;
matching `--deny-*` flags override them. Piped and transcript sessions never
prompt and deny unflagged effects with E1803. Filesystem access is confined to
the project root descriptor fixed at session start; every later component is
opened descriptor-relative without following symlinks. Platforms unable to
enforce that confinement fail closed. Ambient random draws require `Rand`, but
explicitly seeded `Rng` values are injected data. REPL-owned `print`/`eprint`
capture is inherent and needs no `Io` grant.

Process execution opens the canonical executable before authorization and
launches that exact descriptor without resolving its pathname again. Stdin is
closed unless a future separately authorized stream surface supplies it. The
child starts in the verified project directory with an empty environment;
stdout and stderr are captured. Interrupts forward to its process group, and
the REPL kills and reaps that group after 30 seconds.
Native-only modules still report E1802.

## REPL multiline editing

Raw-terminal `jet repl` uses syntax-aware Enter
(D-FE-REPL-MULTILINE1=A). Enter submits input when the REPL parser accepts its
item, statement, or expression shape. When parsing instead stops at the end of
the current input, Enter inserts a newline and redraws each continuation with
the `· ` prompt. Invalid input that already contains the parser's problem
submits immediately so the normal compiler-owned diagnostic can explain it.

Escape then Enter always inserts a newline, including when the current input
is already complete. Enter on an empty continuation line force-submits. The
editor repaints the whole logical buffer after insertion, deletion, history,
or cursor movement, then restores the cursor to its source position. Cooked
and non-TTY sessions keep D-REPL9's bracket-balance continuation and `...  `
prompt; they do not claim parser-aware raw editing.

## REPL evaluation interruption

In a raw interactive REPL, Ctrl-C during interpreter execution cancels the
current turn and restores the prompt within 100 ms for Jet-controlled work
(D-FE-REPL-INTERRUPT1=A). The interpreter polls before every instruction and
before and after each runtime call. A blocking external call follows that
call's cancellation behavior; while it remains active the REPL prints
`warning: interrupt requested; waiting for active external I/O to stop`.

Cancellation is transactional for session state. Bindings, moves, and
statement history from the interrupted turn do not commit; earlier session
state remains. Host effects completed before cancellation cannot be undone,
so the REPL prints `Interrupted. External effects already performed were not
rolled back.` The interrupted turn remains visible as `interrupted` in
`:turns` and can be replayed with the ordinary rerun mechanism. A second
Ctrl-C received while that turn is still stopping exits the REPL. Outside
evaluation, Ctrl-C keeps its editor behavior: clear nonempty input first;
exit from an empty prompt.

## REPL history

The REPL keeps the latest 2,000 successful submissions between sessions
(D-FE-REPL-HISTORY1=A). Failed turns and meta-commands are not stored. History
lives at `$XDG_STATE_HOME/jet/repl-history` on XDG systems or the platform
state-directory equivalent. Its directory and file are owner-only. Each input
is stored losslessly, including multiline, effectful, or secret-bearing text;
Jet cannot truthfully identify every secret.

State-path traversal rejects symlink/reparse components and holds the opened
history directory as the authority for later reads, replacements, and erasure.
Each write and clear takes a bounded, crash-released cross-process lock, then
re-reads current history before changing it. Concurrent sessions therefore
merge successful submissions, and a stale session cannot resurrect entries
removed by `:history clear`. Replacement is atomic and durable on supported
platforms.

F3 opens interactive history search. `:history search <text>` is the textual
path and `:history clear` erases the whole file. `JET_REPL_HISTORY=off` keeps
history in memory for the current session only. `JET_REPL_HISTORY_LIMIT=N`
changes the retained-entry bound. If the file ends in a corrupt or incomplete
record, the REPL discards that tail, preserves the valid prefix, and warns. If
private storage cannot be opened or written, the REPL warns and continues with
session-only history.

## Editions & release policy (E2-M2)

A project pins an **edition** with `edition: "2026"` in its `pkg.jet`
(D-REL3). An edition opts the project into a specific era of Jet syntax; the
toolchain advertises the editions it supports in `jet --version` and rejects a
future edition it can't provide (E2001). Single-file `jet run file.jet` carries
no edition marker and always uses the newest stable edition (E2-V4). The full
compatibility contract — patch/minor/major/epoch/edition definitions, the
backward-compatibility guarantee, the deprecation window (L2001 → E2002), the
migration authority (only `jet fix` + edition upgrade, D-REL5), and the
generated-code license statement — lives in docs/spec/release-policy.md.

## Toolchain as a dependency — the `jet:` pin (D-JPK-TOOLCHAIN1=A, #179, U30)

A `pkg.jet` pins **which Jet compiler** builds the project with a `jet:` field
in `payload`, whose value is a **channel ref** (D-JPK-CHANNEL1 semantics):

```jet
payload: {
    name:    "wordstats",
    version: "0.3.1",
    jet:     0.4,          // track the 0.4 series
}
```

Accepted forms: a `MAJOR.MINOR` series (`0.4`), a `MAJOR.MINOR.PATCH` exact
(`0.4.2`), or a named channel (`main`). A range/operator form (`>=1.0.0`) is
**not** a pin — it is the legacy compatibility constraint (E1208) and stays a
minimum-version gate; a channel-form pin is owned by version dispatch instead
(E1249 rejects a malformed pin). Absent `jet:` = unpinned: the running `jet`
builds it with no fetch (rung-0/1 stays frictionless).

The channel resolves to an exact version recorded in the `.jet/lock`
`[[toolchain]]` block (channel + version + envelope). The channel re-resolves
only on `jet update jet` and first realization; every other run reads the lock.
A running `jet` in the pinned channel builds the project natively. A `jet` from
a different series **realizes the pinned compiler as a prebuilt hangar object**
(D-JPK-CACHE1 substitution) and **re-execs into it** (D-JPK-DISPATCH1) — never a
source build of the compiler; a platform cache miss is E1251, never a silent
wrong `jet`. A `JET_TOOLCHAIN_EXEC=<version>` env marker guards the re-exec so
the pinned child runs natively without looping. Under `--offline`/CI an unlocked
channel is E1250 (run `jet update jet`, commit the lock). Verbs: `jet self toolchain`
(read-only pin/version/status), `jet update jet [<channel>]` (the only place the
pin moves), `jet init` (writes a `jet:` pin for the running channel by default).

This is a *different* toolchain from the Rust/native **build** toolchain that
compiles a user's `extern rust` bridge crates (D-JPK-BUILDTOOL1, E1240): that
one builds bridge dependencies; this one pins the Jet compiler itself.

## Source channels and outdated (D-JPK-CHANNEL1=A, U21)

Source refs may carry channel selectors: `#latest`, `#main`, or a major-series
mask such as `#v0.x`. The channel is tracking intent; `.jet/lock` records the
exact source that intent resolved to:

```toml
[[source_channel]]
name = "default"
channel = "latest"
exact = "github:acme/tool#v1.2.0"
```

`jetpack update [source]` is the only verb that moves `[[source_channel]]`.
`jetpack outdated` compares the lock to channel metadata and writes nothing.
`jetpack build`, `jetpack run`, `jetpack enter`, and `jetpack dev` read only the
exact lock entry; an unlocked channel source is E1271, including under CI or
`--offline`.

### Frozen-forward identity block

The `payload:` block's `name`, `version`, and `jet` fields form the project's
**identity block**, read by a dedicated pre-parse (`Jetpack::JetPin::
identity_preparse`) *before* the full manifest parse. Its grammar is
**contract-frozen** and must never be narrowed, so version dispatch can never be
wedged by later manifest evolution (the Go `go.mod` contract):

- The reader finds the top-level `payload: { … }` block by brace matching
  (strings skipped), then extracts `name:`, `version:`, and `jet:` as simple
  `key: value` entries at the block's top level, unquoted and trimmed.
- Any other top-level key, any unknown nested block inside or outside `payload`,
  and any surrounding syntax the running `jet` doesn't recognise is tolerated
  and skipped — it never blocks the identity read.

Guarantee: **every past and future `jet` can read the identity block of any
`pkg.jet`.** New manifest features may only *add* fields/blocks the identity
reader ignores; the three identity fields keep this exact `key: value` shape.

## Typed entry-signature CLI parsing (D-CLIFLAG1, c7cliflag)

The entry function's typed parameter IS the CLI spec — no separate flag
DSL to learn. `fn run()` (S12, zero-arg) is the simple program entry; a program
opts into CLI parsing by defining `fn run` with one parameter:

```jet
@[Cli]
struct ServeArgs {
    @[Doc("port to listen on")]
    #[Default(3000)]
    port: Int
    verbose: Bool
    config: String?
}

fn run(args: ServeArgs) {
    http.serve(routes(), port: args.port)
}
```

`@[Cli]` is a sibling derive of `@[Codable]` on the same marker/derive
machinery (D-MARKERMOVE1). `@[Doc("...")]` is a field-level marker giving
that flag's `--help` line; a field with no `@[Doc(...)]` gets a generic
"value for --name" line instead.

**Entry semantics.** `run` is the only program entry name (S12). It is valid
as either `fn run()` or `fn run(args: T)` where `T` is a CLI spec shape below.
No variadic entry signature exists; raw argv access stays explicit inside
`fn run()` via `core.args`/`core.io.args`. `main` has no entry meaning in Jet.
Bad typed-entry shapes are diagnosed (E1308 below), not silently ignored.

**Pinned field-mapping rule** — every `@[Cli]` struct field maps to exactly
one flag, by this rule (checked top to bottom, first match wins):

| Field shape | Flag | Absent at runtime |
|---|---|---|
| `Bool` | `--name` (boolean flag) | `false` |
| `T?` (`T` a supported scalar) | `--name VALUE` (optional) | `None` |
| scalar with `#[Default(expr)]` | `--name VALUE` (optional) | `expr` |
| any other supported scalar | `--name VALUE` (**required**) | runtime error, `core.args` voice — no new diagnostic code |

Supported scalars: `Int`, `Float`, `Bool`, `String`, `Path`. Any other field
type (a `[K: V]`, a closure, a `[T]`, a nested struct that isn't itself
`@[Cli]`, …) is **E1305** — there is no flag shape for it. Field defaults
use the *existing* `#[Default(expr)]` marker (D-SERDE5) — not a second,
inline `= expr` mechanism (that syntax is reserved for function-parameter
defaults, S61, a different grammar slot; reusing `#[Default(...)]` here is
I8: one mechanism for "this field has a default", not two). Field name
`snake_case` → flag `--snake-case` (underscores become dashes); no
casing-style menu (that's a wire-format concern, D-SERDE3, not a CLI-flag
one). No positionals are derived from struct fields in v1 — `core.args`'s
`.positional(...)` builder is the escape hatch for that shape, used
directly (not through the typed layer).

Every generated CLI spec also registers `--help` automatically (rendering
the struct's fields/types/`@[Doc]` text); a field named `help` collides
with it and is **E1306**.

**Nested `@[Cli]` structs are not supported in v1** — a field whose type is
itself a `@[Cli]`-derived struct is E1305, same as any other unmapped type.
(Grouped `--outer-inner` flag prefixing was scoped out rather than bolted
onto the decode machinery under time pressure that would otherwise force a
second, prefix-threaded code path — a real feature, not a punt: it needs
its own worked design before it rides this derive.)

**Subcommands** — an `enum` parameter dispatches by variant:

```jet
enum Cmd {
    Serve(ServeArgs)
    Import(ImportArgs)
}
fn run(cmd: Cmd) { ... }   // $ myapp import data.csv
```

The first positional token picks the variant by its **lowercased** name;
the rest of argv re-parses against that variant's own `@[Cli]` spec (its
own `--help`, its own flags — no flag namespace is shared across
variants). Every variant's payload must be a single `@[Cli]`-derived
struct — any other payload shape is **E1307**. Given **zero** arguments (no
subcommand token at all), the generated entry prints the command list to
stdout and exits 0 — a bare invocation asking "what can this program do"
is treated as a request for orientation, not a mistake; an unrecognized
subcommand name is still a real error (nonzero exit, stderr).

**Codegen** generates directly onto `core.args`'s existing `ArgsSpec`/
`ParsedArgs` builder (D-ARGS1) — the same `.flag`/`.option`/`.parse`
surface a hand-written call chain uses, so there is exactly one parser
(I8), not two. A bad flag at runtime (unknown flag, bad `--port` value, a
missing required flag) is the same `core.args` runtime-error voice as
`ArgsSpec.parse`'s own messages — no new diagnostic codes for that path,
only for the compile-time shape checks above (E1305–E1308). `88_args_spec`/
`64_cli_args`-style direct builder use is untouched; this feature is a
layer generated on top of it, not a replacement.

**Diagnostics:** E1305 (unmappable field type), E1306 (flag-name collision,
including the reserved `--help`), E1307 (subcommand payload isn't
`@[Cli]`), E1308 (`run`'s one parameter isn't a `@[Cli]` struct or an enum
of `@[Cli]` payloads). See docs/spec/diagnostics.md.

**Known limitation:** `@[Cli]`'s generated helper functions use unqualified
`jet_std`/`user_*` paths, so the struct and its `fn run` must live in the entry
file, not an imported module file.

## `jet inspect expand` — transparency command (D-EXPANDCLI1, card #183)

Every "the compiler inferred this for you" mechanism (I8: magic default,
expert opt-in) needs a way to ask the compiler what it decided. `jet inspect expand`
is that one command for all of them — never a second, mechanism-specific
CLI flag per feature.

```
jet inspect expand --facts <lens> <file.jet>   # one lens's facts
jet inspect expand <file.jet>                  # every lens, grouped, empty ones skipped
```

Facts are read straight off the ordinary check pass — never a second
analysis, never rustc (I2/I3). A lens renders fields already sitting on the
checked AST (e.g. `Func::is_inline`/`is_inline_always`, validated by the
time the bundle compiled at all) — the same side-channel `jet inspect semindex`/
`jet inspect impact` already read, not a parallel pipeline.

**Floor lenses (this card):**

- `inline` (D-METHODMACRO1) — every fn/method carrying `@Inline` or
  `@InlineAlways`: the contract and the Rust attribute codegen emits
  (`#[inline]` / `#[inline(always)]`). Functions with neither marker produce
  no line — the lens reports contracts, not every function in the program.

A `refs` lens (D-REF-SHORTHAND1) once reported resolved owners for `&T`
stored-reference struct fields; D-MEM1/S3 deleted that mechanism outright
(no stored-borrow fields in v1), and the lens went with it — `jet inspect expand
--facts refs` is an unknown-lens usage error today, like any retired name.

Unknown `--facts <lens>` lists the registered lenses and exits nonzero
(usage error, not an E-code — it never reaches the diagnostic renderer). A
file that fails to compile prints the ordinary front-end diagnostics and
exits nonzero: facts require a clean check, same as `jet inspect semindex`/
`jet inspect impact`. A clean program with no facts for a lens (or for every lens,
bare form) exits 0 — absence of facts is not a failure.

**Extensibility:** lenses live in one static table in `Source/CmdExpand.rs`
(name, one-line summary, renderer) — adding a lens for a future ratified
mechanism (effects, layout, derive expansion) is one row, never a new
subcommand or a new flag (I8).

## Semantic index, dossier, and codemods (D-SEMINDEX1, D-WD2, D-CODEMOD1)

`jet inspect semindex --json <file.jet>` emits schema v3: definitions, references,
call edges, effects, and member facts. Member facts stitch fields, variants,
inline methods, external inherent impl methods, trait impl methods, and trait
requirements into one stable owner-ordered view. Every resolved reference also
carries its definition identity; unresolved or ambiguous references carry no
target and semantic edits must not fall back to spelling. Compiler-internal
structural facts expose checked `expr`, `stmt`, `item`, and written `type` node
boundaries for refactoring tools.

`jet inspect dossier <file.jet> [Symbol]` renders those facts as a human report;
`--json` emits the same lens data. The first shipped lens is the type/member
dossier. It never re-checks by another path and never invents facts missing
from semindex.

The LSP exposes scattered-method breadcrumbs as inlay hints at the owning type
declaration. These are editor-only overlays: they do not edit source and carry
source links to the real impl method spans.

`jet inspect codemod dry-run|apply|undo` uses one replay engine for both schema
versions. A missing version or `version: 1` is the original semantic rename:

```json
{"name":"RenameReport","entry":"main.jet","operation":"rename","from":"report","to":"summarize"}
```

Schema 2 (D-CODEMOD-BATCH1=A) is an ordered batch over typed Jet templates.
`project` is relative to the object. Each root is one `.jet` file or directory
beneath `examples/` or `tests/ui/`; absolute, parent, and symlink escapes fail.
Directory discovery is recursive and byte-path ordered. Rules are either a
semantic `symbol_rename` or an `ast_rewrite` whose `node` is `expr`, `stmt`,
`item`, or `type`. `$value` captures one subtree and `$values...` captures a
list. Matching is confined to the requested compiler-owned AST boundaries;
same token bytes in another node class are not candidates. Symbol definitions
and references are selected by their resolved definition anchors, never by
spelling. Every rule declares its exact match count; duplicate ids, unknown fields,
ambiguous names, unused captures, unresolved replacement names, zero matches,
and overlapping edits fail before any write. Rule N+1 sees a compiler-reindexed
overlay containing rule N, so a later semantic rule may target an earlier
rule's output.

```json
{
  "version": 2,
  "name": "ReportV2",
  "project": "..",
  "roots": [
    {"path":"examples/report.jet","validate":"clean"},
    {"path":"tests/ui/report_type.jet","validate":"fixture"}
  ],
  "rules": [
    {"id":"rename","kind":"symbol_rename","from":{"name":"report","symbol_kind":"function"},"to":"summarize","matches":4},
    {"id":"call","kind":"ast_rewrite","node":"expr","match":"legacy_parse($input)","replace":"parse_int($input, base: 10)","matches":2}
  ],
  "snapshot_after": {"tests/ui/report_type.jet":"migrations/report_type.after.stderr"}
}
```

Clean roots must finish without front-end errors. Fixture roots must reproduce
their complete paired `.stderr` exactly. An intentional snapshot change names
a non-symlink project file in `snapshot_after`; the engine first proves those
bytes equal the compiler-rendered result, then includes the paired `.stderr` in
the same plan, transaction, and undo log. Code-only fixture changes are refused.

Dry-run holds the codemod lock through discovery, staged compilation,
validation, input rehash, and diff output but writes no source, snapshot, log,
temporary file, or journal. Apply requires `--yes` after warning that an editor
which ignores the codemod lock can still race the final rename. Apply writes
same-directory temporary files, fsyncs contents and parents, and advances a
fsynced recovery journal around each rename. Replacement reopens each parent
without following links, verifies the destination through that handle, and
renames relative to the same handle (`openat`/`renameat` on Unix; directory and
file handles plus `SetFileInformationByHandle` on Windows). The process lock is
an OS-owned advisory lock on Unix and a delete-on-close exclusive file on
Windows, so crash recovery does not depend on `/proc`. A later codemod recovers a crash
before planning; unexpected concurrent bytes preserve the journal and stop.
Schema-2 logs contain byte-exact before/after images. Undo verifies every
after-hash before making any write and uses the same journal protocol to restore
source and snapshots. Existing schema-1 inverse-edit logs remain readable.
Unified dry-run output emits the standard no-newline marker for each side whose
last line lacks a terminating newline.

## Web dev-server dashboard (D-FE-DEVSRV1)

`jet dev <file.jet> --target=web` exposes one shared status snapshot at
`/__jet_dev_status`. The terminal dashboard and browser corner strip render the
same status words, client count, build time, and diagnostic. Browser clients
have tab-scoped identities with a short polling lease, so the count represents
live tabs rather than transient HTTP connections.

In a TTY, the terminal keeps a two-row header pinned above the scrolling log.
Pressing `v` toggles request and rebuild detail without changing the shared
status; `--verbose` starts with that detail open. The scroll region is installed
only after raw input and its cleanup guard are active. `NO_COLOR` replaces the
status dot with a bracketed state word while retaining TTY pinning and controls.
Non-TTY output is plain and append-only.

While rebuilding, the browser dims the last good page. A failed build expands
the strip into an overlay containing the front end's verbatim diagnostic and
keeps serving the last good artifacts. `Esc` collapses that diagnostic without
hiding the error status. The next clean build clears it and reloads. A failed
status poll shows reconnecting in the browser; expiry of its server lease puts
the terminal on the same reconnecting state. The renewed lease returns both to
the underlying build state. Reconnecting overrides ready, building, and error
on both surfaces while retaining the last build time and any diagnostic in the
shared snapshot. The renewed lease reveals that retained state again. Recovery
reloads even when a restarted server reuses the previous process's numeric
version.

## Canvas visual editor prototype (D-BPE-*)

`jet dev <file.jet> --target=web` serves Canvas at `/canvas` (with the same
versioned JSON endpoints also reachable under `/__jet_canvas`). Canvas is a
projection of checked Jet source, not a graph asset. `/__jet_canvas/graph`
emits `jet.canvas.graph` schema v1 with one function graph per checked function:
deterministic source-order layout, structural nodes, typed pins, data/fallible
wires, inline pure expressions, source byte spans, and semindex fact handles.

`POST /__jet_canvas/transaction` accepts `jet.canvas.edit` schema v1.
Transactions include source no-op/reprojection, rename, inline expression edit,
binding promotion, call insertion, function/signature edits, trait impl creation,
wire break/move, source replace, structural rail inserts, comment/collapse
regions, and action preview. Each transaction must carry the current source
`revision`; stale revisions fail with a conflict. Successful writes go through
`jet fmt`, re-check through the front end, replace ordinary `.jet` source, and
then reproject.

The Code lens is read-only by default. `Edit Source` switches to an explicit
source editor, and `Apply Source` sends a `replace_source` transaction through
the same format/check/reproject path. Canvas never writes a graph asset or owns a
second parser/checker.

`POST /__jet_canvas/query` accepts read-only query schema v1 for find,
references, source-to-graph, rename preview, action palette, and Core catalog
browsing. `GET /canvas/core-catalog` exposes the same read-only `core.*`
catalog from the canonical Core library reference. Catalog entries carry
`canvas.catalog:core.read` authority and `writes:"none"`; browsing never claims
that a Core call executed.

`GET /canvas/proof` reports the selected source revision's current proof state:
front-end check result, Git text state, local debug persistence, and command
receipt status. Missing build/run authority receipts are reported as missing and
stale; Canvas never converts a graph projection into proof that code ran.
The Run button opens the real `jet run <source>` command authority card; it does
not simulate output. `POST /canvas/command` executes only whitelisted
run/check/build authority cards for the current revision, records the receipt,
and lets the proof rail mark that exact revision current. Build output commands
require explicit confirmation.

The public v1 graph/edit field contract is pinned in
[`docs/reference/canvas-protocol.md`](../reference/canvas-protocol.md), and the
AST-derived Canvas coverage ratchet is pinned in
[`docs/reference/canvas-parity.md`](../reference/canvas-parity.md).
Unknown request fields are ignored by v1; unknown operations fail as Canvas edit
errors, and unknown future graph fields may never carry hidden semantics.

## Public front-end toolkit API (D-FRONTENDAPI1=A, card #227)

The public compiler toolkit is a read-only value facade over the front end.
Rust dogfood tools use `jet::Compiler`; the exposed shapes are stable data,
not AST handles or mutable compiler state.

Version 1 exports:

- `lex_source(src)` → token views with stable kind strings, byte ranges, and
  line/column positions.
- `parse_source(src)` → top-level syntax summaries plus diagnostics.
- `check_file(path)` → diagnostics, syntax summaries, and a semantic-index
  snapshot when the file checks cleanly.
- `source_map_from_generated_rust(rust)` → generated Rust line markers mapped
  back to Jet source lines.

Diagnostics are cloned into value records (`code`, severity, message, why,
fix, span). Semantic facts are cloned from the existing semindex schema. No
API returns `Program`, `Item`, `Expr`, `Token`, mutable caches, parser state,
or sema internals, and no API can feed modified syntax back into compilation.

## Inline script dependencies — `use pkg#version` (D-JPK-SCRIPTDEP1=A, U11)

A bare `.jet` script — no `pkg.jet` — may open with an inline dependency
instead of a manifest:

```jet
// stats.jet
use textkit#1.4

fn run() {
    print(textkit.wrap(input(), width: 72))
}
```

`pkg#version` is the `#` directive plane's version-selector form
(D-MARKER-FAMILY1); the version is a dotted numeric selector (`1.4`, `1.4.2`),
never a range operator. Only a single-segment module name takes one — `use
core.files#1.0` is nonsensical and isn't accepted.

`jet run stats.jet` collects every inline ref from the entry file, resolves
each, and wires the resolved directory into module search exactly like a
hangar-realized `library` (U17) — the rest of import resolution is
unchanged. Resolution is a local directory lookup only (no network, no
code execution): a script's own `.jet/inline-deps/<name>/<version>/` copy
(the `.jet/` managed-folder convention), or a version matching one there by
dotted prefix (`1.4` matches `1.4.2`). Consuming the public Jet package
registry by name isn't wired yet (E1207/M12.2 — publishing today writes only
the sparse index line, never a fetchable source tree); an inline ref that
resolves nowhere is E1253.

A loose selector (`1.4` rather than an exact `1.4.2`) is fine to write —
rung 0 stays magic — but is L0203: nothing pins it until `jet store lock stats.jet`
writes a `stats.jet.lock` sidecar (`script_hash` + each dep's resolved
version and content hash, keyed by the script's own file-content hash so an
edit goes stale). `jet init stats.jet` lifts the inline refs into a freshly
written `pkg.jet`'s `deps: {}` block, growing the script from rung 0 to rung 1
(vision.md's ladder) without discarding what it already declared.

## `target: plugin` — sandboxed WASM Component Model plugins (c81, D-PLUGIN1=B, D-DEP-WASM1=A)

A package built `target: plugin` compiles to a sandboxed `wasm32` Component
Model module instead of a native binary. A host program loads and calls it —
safe by default, **no `#Unsafe` gate anywhere in the story** (I1): the
sandbox is the safety boundary, by construction. This is a general
application-plugin substrate, distinct from the deferred Epoch-3
compiler-extension plugin API (custom lints/sema hooks,
`docs/plans/epoch-3/plugin-api.md`) — don't conflate them (I8).

```jet
// pkg.jet
payload: { name: "mathkit", version: "0.1.0" }
```

```jet
// main.jet — the plugin's own source, no `fn run()` (it's loaded, not run)
pub fn scale(a: Float, b: Float) -> Float {
    return a * b
}
```

`jet build main.jet --target=plugin` writes a `.wit` world (generated from
the entry file's top-level `pub fn` surface) plus the wasm32 guest Rust, then
shells out to `rustc --target wasm32-unknown-unknown --crate-type cdylib`
followed by `wasm-tools component embed`/`component new` (D-DEP-WASM1=A) —
external CLI tools, never linked into the compiler (I6) — producing a
`.wasm` Component Model binary.

A host is an ordinary native Jet program:

```jet
use core.plugin as plugin

fn run() {
    mathkit :: plugin.load("mathkit.wasm")
    area :: mathkit.call("scale", [6.0, 7.0]) ?? panic("scale failed")
    print("scale(6, 7) = {area}")
}
```

`Plugin.load(path) -> Plugin` produces a handle (mirrors `core.db`'s
`open`/`open_memory`); `.call(name, [Float]) -> Float ? String` and
`.call_int(name, [Int]) -> Int ? String` are the only instance methods (v1
scope — every parameter and the return type must be all-`Int` or all-`Float`,
E1260; Bool is a trivial follow-on, Text needs the Component Model's
memory-based string ABI, a real future increment). The wasmtime host embedded
via the FFI-bridge pattern (`crates/jet-driver/src/Prelude/Plugin.rs`,
runtime-side only, I6) registers **zero host imports** — deny-by-default
capabilities: a plugin that tried to touch the filesystem, network, or clock
simply fails to instantiate at load time, reported as a clean `Err`, never a
crash (I2). A plugin's own code may not use any effect either — caught at
build time as E1258, not deferred to that runtime failure.

D-PLUGIN-EXPORT1=A: the exported surface is named by the manifest `export:`
target field (`plugin { export: "mathkit" }`), defaulting to the package
name. D-PLUGIN-VERSION1=A: the exported interface is frozen via
`Sema::ApiFreeze`'s pub-metadata semver-snapshot mechanism (the same one an
ordinary library's public API uses, E1218/E2601) — keyed `plugin__<export>`
in `.jet/cache/api/` so it never collides with a library's own frozen API in
the same project. Rebuilding a plugin with an unchanged interface freezes
silently; removing or changing an export is E1257, naming the exact delta.

Full worked example: `examples/features/packages/plugin_mathkit/` (a
`plugin_src/` package + a host `main.jet`; golden-enforced, I5). New
diagnostics: E1257 (interface changed incompatibly), E1258 (capability
denied), E1259 (wasm build/toolchain failure), E1260 (unsupported export
shape) — see docs/spec/diagnostics.md.

## Programmable builds as Jet (D-BUILDENTRY1 and build-graph decisions)

`jet build` checks the root program, then runs one optional root
`fn build(b: BuildContext) -> BuildPlan ?` through the same interpreter used by
comptime. Imported `fn build` declarations are checked but never run. With no
root entry, the existing zero-configuration pipeline is unchanged.

Build code registers ordinary typed values. Targets are declared once with
`b.add_executable`, `b.add_library`, `b.add_test`, `b.add_bench`,
`b.add_asset_bundle`, `b.add_doc`, `b.add_install`, `b.add_package`, or
`b.add_publish`; each returns a `BuildTarget`. `b.action(name, inputs, outputs,
argv, caps)` returns a `BuildAction`. `b.plan()` or `b.plan(default)` hands the
same canonical graph to scheduling, caching, execution, query tools, and the
LSP. Expert actions may additionally pass a typed `BuildToolchain` and a list
of typed `BuildProbe` handles as arguments six and seven. `b.toolchain(name,
target_triple)` records the target identity; `b.probe(name, kind, value)`
supports `find_program`, `pkg_config`, and `header` probe kinds.

```jet
fn build(b: BuildContext) #(Exec, Fs) -> BuildPlan ? {
    #Impure("run declared toolchain probe and action") {
    shell :: b.probe("shell", "find_program", "sh")?
    native :: b.toolchain("native", "x86_64-linux")?
    stamp :: b.action(
        "stamp",
        ["assets/version.txt"],
        ["build/version.txt"],
        ["sh", "-c", "cp assets/version.txt build/version.txt"],
        ["Exec", "Fs"],
        native,
        [shell]
    )?
    app :: b.add_executable("app", ["main.jet"], [stamp])?
    return b.plan(app)
    }
    return b.plan()
}

fn run() { print("hello") }
```

Action dependencies come from target dependencies and declared file
producer/consumer edges. Ready nodes run concurrently in deterministic stages;
`linker`, `console`, `gpu`, and named resource pools serialize. Cached actions
key declared input content, argv, environment, capabilities, toolchain, probes,
resource pools, plugins, and generated-source hashes. Cache hits restore only
declared outputs from the local CAS.

Execution has no ambient fallback. On Linux each action runs under bubblewrap
with private mount, PID, IPC, UTS, and network namespaces. Only declared inputs
enter its writable work tree; only declared outputs return. Network remains
unshared unless both source and policy grant `Net`. A missing sandbox is E3505,
not an unsandboxed run. Single-file authority uses the ratified per-effect
flags (`--allow-exec`, `--allow-fs`, `--allow-net`, and the remaining D-EFF4
names). Package/workspace policy can grant or cap the same capability set.
The vocabulary is one closed typed ten-effect enum shared by sema, policy,
CLI, graph, cache, and executor. Every effectful `b.action`/`b.probe` call must
be inside its active `#Impure("reason")` region. Signature declaration and
effective grant are checked before any probe or process runs.

`b.generate(name, source)` materializes `.jet/generated/<package>/<name>.jet`.
Action outputs ending in `.jet` follow the same path. Both re-enter lexer,
parser, and sema before runtime codegen; malformed generated source is a Jet
diagnostic with generator provenance. The build-only entry and imported build
entries are removed before codegen, so rustc never sees build handles.
The selected target source/dependency closure plus generated modules becomes a
fresh runtime bundle. Native, cross, web, plugin, and freestanding lowering all
consume that same checked bundle. `--locked` compares generated input/output
hashes before committing provenance; drift is E3512 and action outputs roll
back.

`jet inspect graph <file> --json` and `jet inspect query build <file> --json` return the same
typed graph without executing actions. `jet inspect explain-build <target|action|file>
<file>` reports graph and cache provenance. LSP checking uses the same selected
root signature validation and the same static graph facts, including E3501.

## Deliberately absent

See non-goals in docs/spec/philosophy.md. The parser should produce staged
or guiding errors for the ones users will reach for (e.g. `and` → teaching
error naming `&&`, per S14).
