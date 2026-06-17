# To-Do List

- Change import to use
- Support for labeled loop "blocks"?
- Ensure we support multiple constructor types
- Relook module implementation & pack.jet @docs/plans/jetpack-jetos/unified-ecosystem.md
- Support H file imports for c ffi
- Named + default arguments: Swift, Kotlin, Gleam (labels), Python, C#, Ruby. Big readability/beginner win. (§23)
- REPL Support
- Pipelines (|>): F#, Elixir, Gleam, Elm, OCaml, Julia. (§15)
- A cleanup primitive (defer/errdefer): Go, Zig, Odin, Swift, Nim, Hare. Recurs constantly; pairs naturally with transact. (§0.1)
- Optional-chaining / unwrap ergonomics (?., ??, guard/if let): Swift, Kotlin, C#, Dart. Jet has T?/or; round it out. (§12)
- Digit separators in numeric literals (1_000_000): Julia, Rust, Swift, Kotlin, Go, Ada, many. Free readability win. (§34)
- Atomic/transactional rollback (owner-flagged): Verse. (§0.1)
- Purity tracking (pure/func vs impure): Flix, Nim, Koka, D. Confirms S60. (§5)
- Content-addressed artifacts (not identifiers): Unison, Nix. Feeds jetpack. (§0.2)

### 2. A debugger

DAP source maps are currently "deferred past v1.0" — for industry this
is the wrong shelf. No enterprise team ships a language its developers
cannot step through. Since Jet transpiles to Rust, the pragmatic v1 is
line-directive-style source mapping so gdb/lldb/VS Code show Jet source
lines, not generated Rust. Recommendation: promote to the
committed-additions list in docs/spec/roadmap.md.

### 3. Supply-chain features in M12 Phase 2

When the registry lands, enterprises will require:

- Private/internal registries and mirror support (Artifactory/Nexus
  proxying).
- Vendoring for air-gapped builds.
- SBOM emission (CycloneDX/SPDX — nearly free given the lockfile).
- Namespace ownership rules.
- An advisory database and a `jet audit` command.

None of this conflicts with the existing M12 design; it is mostly
Phase 2 scope.

### 4. Observability stdlib

M10 has fs/io/json but nothing for production operations. Minimum bar:
structured logging in `std/log`. Eventually metrics and trace-context
propagation — but logging alone covers most CLI/tool use cases, and it
should exist before anyone runs Jet in production.

### 5. A server-side story

Committed-addition item 5 (blocking sockets + HTTP *client*) covers
tools, but enterprise bread-and-butter is services:

- An HTTP **server**.
- TLS — bridge to rustls via the FFI tier; never hand-rolled.
- Database connectivity (Postgres first; FFI to a vetted Rust driver).

This is also where "no async, tasks + channels only" gets
stress-tested. Thread-per-connection is fine for internal services at
hundreds of connections; hold that line for v1.x rather than reopen
async, but write the positioning down explicitly ("Jet services scale
like Go circa 2012; if you need 100k connections, that's not us yet").

### 6. Cross-compilation surfaced as a feature

rustc provides the target matrix nearly free — `jet build --target
linux-arm64` would be a one-flag enterprise feature (build on CI for
the deploy target) that is mostly inherited. Cheap to add to M6 or M14
scope.


Consider The Following: 
1. Transparent alias — a second name for the same type. No new type; the compiler treats them as identical. Used to make long types readable:
type OrderBook = Map<String, [Order]>;   // alias
fn settle(book: OrderBook) -> Money { ... }   // vs Map<String, [Order]> everywhere
Liked: shortens noisy generic types, documents intent at a glance. Disliked: can over-abstract — a reader sees OrderBook and has to jump to its definition to learn it’s “just a map”. Because it’s transparent, it gives zero extra safety: you can still pass any Map<String, [Order]> where an OrderBook is expected.
2. Newtype (distinct type) — a brand-new type wrapping one value, not interchangeable with what it wraps. Used for safety:
struct UserId(Int);       // (Rust-style) — UserId and ProductId are now
struct ProductId(Int);    //   different types even though both wrap Int

fn ban(u: UserId) { ... }
ban(product.id);          // COMPILE ERROR — can't pass a ProductId
Liked: kills a whole bug class — you can’t accidentally swap two Ints that mean different things (user id vs product id, meters vs feet). Disliked: adds wrapping/unwrapping ceremony, and you often want to forward some operations (arithmetic on a Meters) which means writing trait impls.
