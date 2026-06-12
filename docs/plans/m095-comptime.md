# M9.5 — Comptime v1 (CTFE)

**Decisions:** S26 (value-level comptime, ratified 2026-06-12), S57
(`comptime x = f();`), and S55 (hybrid derives, M9) ratified. Derives are
M9 comptime-layer-2, not this milestone;
S56 (user derives / typed reflection) is deferred past v1.0. Depends on
M9 (the interpreter must see the final v1 type system, generics
included).
**Error codes:** E0951+.

## Goal

Run ordinary Jet at compile time and paste the answer into the binary.
**One law (S26): comptime never creates, parameterizes, or selects a
type, and never affects dispatch** — polymorphism belongs to traits
(M9); comptime computes *values*. There is no comptime sublanguage and
no annotation on callees: any pure Jet function is comptime-callable
as-is. The pitch in two sentences: "`comptime` runs this code while the
program compiles and bakes the answer in. If the code panics, that's a
compile error with your message."

## Surface (ratified S57)

```jet
fn make_table() -> List<Int> {
    var t = [];
    for i in 0..255 {
        t.push(i * i % 251);
    }
    return t;
}

comptime TABLE = make_table();           // evaluated during compilation
comptime MOTD = embed_file("motd.txt");  // file baked into the binary

fn main() {
    print("{TABLE[10]} — {MOTD}");
}
```

- `comptime name = expr;` — top level or local. `comptime` is itself the
  binding keyword; the binding is always immutable (it is a constant),
  so no `val`/`var` follows. `comptime val` / `comptime var` / `const`
  get teaching errors (S14 machinery, E0954).
- `embed_file("path")` builtin: path relative to the file's directory
  (S16 convention), returns `String` in v1 (byte version waits for M10
  `U8` buffer APIs). Only callable from comptime-reachable code.
- No comptime blocks, no comptime parameters, no comptime types (the
  S26 law + smallness). Revisit blocks post-1.0 only with evidence.

## Sema rules

1. **Check first, run second.** The initializer is type-checked as
   ordinary Jet under every existing rule, *then* evaluated by a
   tree-walking interpreter over the typed AST. This ordering is the
   diagnostics guarantee: no Zig/C++-style instantiation-time error
   class exists, because nothing untyped is ever evaluated.
2. **Purity.** The comptime-reachable call graph may not touch IO,
   `extern rust` (M7), `tasks.*` (M11), random, or time. Violation →
   E0951, naming the offending call *and* the call path that reached it
   ("`make_table` calls `helper`, which calls `fs.read` — files can't
   be read while compiling; `embed_file` can").
3. **Fuel.** Step budget (default 10M ops) and memory budget (default
   64 MB) per binding; exhaustion → E0952 with a comptime call trace
   ("this loop had run 4,012,332 times when compilation gave up").
   Budgets are compiler-internal v1 constants, not user knobs
   (philosophy: minimal configuration).
4. **Comptime panic is a feature.** `panic`/`require` during evaluation
   → E0953: "your comptime code panicked while compiling" + the user's
   own message + call trace. Document as the sanctioned way to write
   custom compile-time validation (the proc-macro use case).
5. **Results are ordinary values.** A comptime binding behaves exactly
   like a `val` of its type at every use site — M2 ownership rules,
   clone-by-rule, no special cases. Top-level comptime bindings are the
   first module-level bindings in the language; they are constants, not
   global mutable state (still a non-goal).
6. **Bit-for-bit semantics.** The interpreter implements runtime
   semantics exactly: `Int` overflow policy identical to compiled
   output, IEEE f64 for `Float` (including S21 display), char-counted
   strings (S41), map ordering (S38/BTreeMap). Enforced by the
   differential battery below — divergence is a P0 miscompile-class
   bug, not a diagnostic bug.
7. **No silent folding.** Sema never const-folds non-`comptime` code
   (S57 rejected option C); optimization of runtime code remains
   rustc's job (R2). Predictability beats cleverness.

## Codegen lowering

| Jet                              | Rust                                                         |
| -------------------------------- | ------------------------------------------------------------ |
| `comptime X = …;` (Int/Float/Bool/Char) | `const user_X: i64 = <literal>;` etc.                 |
| `comptime X = …;` (String)       | `const user_X: &str = "<literal>";` (+ `.to_string()` at use sites sema marks) |
| `comptime X = …;` (List/Map/struct/enum) | one-time-initialized static built **from literal data only** (`LazyLock` + literal constructors); no user code runs at runtime |
| `embed_file("p")`                | the file's contents as a string literal — the file is never opened at runtime |
| local `comptime x = …;`          | a literal in place                                           |

Codegen stays dumb (I3): it serializes already-computed values to
literals; it never evaluates anything. rustc sees only constants it can
trivially verify (I2 risk ≈ zero).

## Diagnostics to register (docs/04)

E0951 comptime code reaches an impure operation (shows the call path) ·
E0952 comptime budget exhausted (call trace + loop counts) · E0953
comptime panic = user-authored compile error (user's message verbatim) ·
E0954 teaching: `comptime val` / `comptime var` / `const` → `comptime
x = …` · E0955 `embed_file`: missing / unreadable / not UTF-8 (path
shown relative to the importing file).

## Examples & tests

- `examples/27_comptime_table.jet` — baked lookup table; golden output
  proves identical results to a runtime-computed copy.
- `examples/28_embed.jet` — `embed_file` + a comptime `require`
  validating the embedded content (with a `.fixed.jet` companion for
  the failing variant).
- ui fixtures for every E095x, including the E0951 call-path rendering
  and the E0953 user-message passthrough.
- **Differential battery** (`tests/comptime_diff.rs`, permanent CI):
  each fixture expression is compiled twice — once as `comptime X = e;`
  and once as runtime `val x = e;` — and the program prints both;
  goldens assert byte-identical output. Coverage axes: Int overflow
  edges, Float rounding + S21 display, String/Char ops (S41), List/Map
  ordering, nested struct/enum values. Any divergence fails CI as P0.
- Fuel fixture: an infinite comptime loop must fail in bounded time
  with E0952 (also protects the future LSP, M13).

## Out of scope

Comptime blocks/params/function annotations, comptime types & const
generics (S26 law), user derives & reflection (S56), comptime IO beyond
`embed_file`, cross-build evaluation caching (perf work, later), user
budget knobs, `embed_file` byte mode (M10).
