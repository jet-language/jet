# Jet language specification

This specification states Jet's durable language semantics, safety model, and
design boundaries. Code and executable registries are the source of truth for
exact spellings, APIs, diagnostics, and current behavior:

- **Keywords, sigils, and grammar:** `Compiler/JetFoundation/Data/Syntax/Spellings.jet`
- **Type & semantic checks:** `Compiler/JetSema/Source/`
- **Diagnostics:** `Compiler/Prelude/Diagnostics.jet` and `Tests/UI/`
- **Executable examples:** `Examples/` and `Tests/Cases/`
- **Core APIs:** `Core/` and `Compiler/Prelude/`
- **Ratified decision history:** Tower (`Tools/Tower/.tower/`) and `Docs/Spec/syntax-decisions.md`

Use the [Jet vocabulary](vocabulary.md) for domain terms. (Full historical
8,290-line prose lives in git history: `git show 20be9d938a5:Docs/Spec/spec.md`.)

## Lexical structure and core grammar

- **UTF-8 source, newline-terminated statements:** Statements terminate at
  newlines; semicolons are not required. Comments use `//` and `/* ... */`.
- **Bindings & types:** Bindings use `name: expr` (immutable by default) and
  `^name: expr` (mutable). Declarations use `Name: struct { ... }`,
  `Name: enum { ... }`, `Name: trait { ... }`, and `name: fn(...) -> Ret E! { ... }`.
- **Markers:** Every compiler/declaration marker uses `#` (`#Test`, `#Error`,
  `#Unsafe("reason")`, `#CLI`, `#Meta`, `#Todo`, etc.); `@` is reserved for
  locations/sources; `$` is the compile-time stage mark.
- **Formatting:** `jet fmt` enforces one canonical formatting style with no
  style-dialect knobs.

## Values, expressions, branching, and loops

- **Literals:** Struct literals are dotless (`Point{x: 1, y: 2}`); enum
  variants use leading-dot shorthand (`.Some(v)`) or qualified form
  (`Type.Variant{label: v}`); lists (`[T]{...}`), maps (`[K:V]{...}`), and
  fixed-size lists have explicit typed literal forms.
- **Lexical shadowing & calls:** Lexical value bindings supply their own call
  contract and shadow file-level functions cleanly. Public argument labels and
  call zones are part of callable type identity (mismatch is a compile error).
- **Branching & pattern matching:** `if cond { ... } else { ... }`, multi-arm
  `if { cond -> ... else -> ... }`, and value matching `if expr == { .A -> ... }`
  are the single branching construct (`when` is retired). Exhaustiveness is
  enforced in sema.
- **Loops:** `loop { ... }`, `loop item in iter { ... }`, and
  `loop (i, item) in iter { ... }` with `break` and `next` (not `continue`).
- **Intentional discard:** Discarding a pure non-`Unit` result is a lint
  (`L0508`); `.drop("reason")` documents intentional discard.

## Types, generics, traits, closures, and dimensions

- **Nominal types & methods:** Structs and enums have nominal identity; methods
  attach through `impl Type { ... }` and trait implementations through
  `impl Type.Trait { ... }`.
- **Fixed-size lists & unions:** Fixed-size arrays carry compile-time length in
  the type; heterogeneous values use checked closed unions (`[A | B]`) or trait
  implementations—never an untyped `Any`.
- **Closures & function values:** Function values preserve full parameter
  labels, zones, effects, and error domains; unannotated function values never
  erase strict callable contracts.
- **Physical dimensions:** User-defined and Core physical dimensions compose
  statically at compile time with zero runtime overhead.

## Errors as values

- **Explicit error domains:** Fallible functions declare their error domain
  (`T E!`; infallible functions use `T Never!`). A list of `#Error` types forms
  a failure domain.
- **Propagation & fallback:** `expr ?? fallback` handles or converts errors;
  `expr ?? return Err(...)`, `expr ?? break`, and `expr ?? next` control flow
  directly. Unhandled entry errors format through the shared report boundary
  and exit with code `1`.
- **Panics:** `panic`, `require`, and `#Todo` are program faults (exit `70`),
  not ordinary error handling.

## Ownership, borrowing, access sigils, and memory safety

- **Safe by default (I1):** Safe Jet is memory-safe, type-safe, and leak-free
  with deterministic destruction and no garbage collector.
- **Access sigils:** Parameters and bindings distinguish read-only borrow,
  mutable borrow (`^`), and ownership transfer explicitly at declarations and
  call sites.
- **Expert memory tier:** Arena/custom allocators, explicit layout control, and
  raw memory operations live behind explicit opt-in APIs; raw unsafe operations
  require audited `#Unsafe("reason") { ... }` blocks or `#Unsafe("reason") fn`
  tracked by `Docs/Spec/safety.md`.

## Effect system and authority

- **Checked & erased effects:** Functions declare side effects in their
  signature (`-[FS]>`, `-[Net]>`, `-[IO]>`, etc.); pure functions omit the
  effect clause. Effects are statically checked in sema and erased at runtime
  (no runtime effect handlers or monads).
- **Authority & packages:** Capabilities/rights are declared at package and
  boundary levels. A standalone `.jet` file may begin with one optional inline
  `package: { ... }` carrier (byte-zero `#!/...` shebang allowed before it),
  parsed by the exact same `PackageFacts` model as `package.jet`.

## Imports, visibility, and configuration

- **Explicit module aliases:** `use alias: module.path` binds qualified access;
  Core modules live under `core.*`.
- **Visibility:** Private by default; `pub` exports publicly; `priv(package)`
  scopes visibility to the enclosing package.
- **Composable configuration:** Typed configuration modules evaluate
  deterministically without ambient mutable global state.

## Concurrency

- **Static data-race freedom (`D-DATARACE1=C`):** Safe Jet rejects data races
  at compile time across tasks, channels, and reactive state (`Signal`,
  `Derived`, `Computed` use lock-ordered `Arc` storage).
- **Structured tasks & channels:** Tasks run within structured scopes with
  deterministic cancellation and cleanup; typed channels hand back unsent
  values in send errors when refused or closed.

## Compile-time execution and metaprogramming

- **One native meaning (I9):** Compile-time code (`$`-staged items and comptime
  evaluation) runs natively at O0 in a child of the compiler with the exact
  same semantics and Core runtime as runtime code.
- **Inspection & reflection:** `reflect.of(x)` exposes static type metadata;
  `jet inspect expand` shows deterministic compile-time expansion output.

## Foreign-function interfaces, web, and sandboxing

- **FFI boundaries:** Foreign calls cross explicit typed declarations and
  audited `#Unsafe("reason")` boundaries; importers never silently discard
  unsupported foreign constructs.
- **Browser & web targets:** Web/browser effects and values share Jet's effect
  and error model across native and web tiers (I9).
- **Sandboxed WASM packages:** WASM guests hold zero ambient host authority;
  every host capability is an explicit, declared import.
- **Programmable builds:** Build scripts are typed Jet programs running in
  explicitly sandboxed actions with deterministic inputs/outputs and no
  unsandboxed fallback.

## Testing, REPL, tooling, and editions

- **Tests:** `#Test("name") { ... }` declares tests runnable through `jet test`
  with golden/UI/case corpora (`Docs/Spec/architecture.md`).
- **Tooling surfaces:** Formatting (`jet fmt`), semantic indexing/codemods
  (`Docs/Spec/Reference/semantic-index.md`), REPL, Canvas
  (`Docs/Spec/Reference/canvas-protocol.md`), command registry
  (`Docs/Spec/Reference/cli.md`), and toolchain/source channels
  (`Docs/Spec/Packaging/`) share one compiler front end and checked model.
- **Inline script dependencies:** Manifest-less inline dependencies resolve
  only from the local managed cache/fixture (never ambient registry lookups).

## Deliberately absent

These are intentional language boundaries, not implied future promises:

- **No `Any` top type or dynamic `any`:** Heterogeneous collections use checked
  unions (`[Int | String]`), traits, or generics; `reflect.of(x)` inspects
  known types.
- **No stored-borrow reference fields** (and no `refs` expansion lens).
- **No nested `#CLI` structs** or implicit grouped flag prefixes.
- **No ambient registry resolution for inline scripts** (fails with `E1253`
  when absent from local cache/fixture).
- **No ambient host capabilities or dynamic export lookup in WASM guests.**
- **No second parser/checker in Canvas** (Canvas owns no graph asset and its
  projection is not execution proof).
- **No unsandboxed build-action fallback.**
- **No runtime effect handlers, monads, or first-class effect values** (effects
  are statically checked and erased).
- **No silent FFI omission inference** outside published importer contracts.
- **No retired-spelling compatibility aliases:** Staged diagnostics teach the
  current spelling (e.g., `and` suggests `&&`) instead of accepting retired
  syntax.
- **No Rust-hosted compiler or Rust emission path:** The Jet-authored compiler
  and native backend are the sole toolchain.
