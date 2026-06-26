# Decision ballots — open owner queue

Every open decision, and **nothing else**. The instant a decision is submitted it
leaves this file: it is recorded in the decision log in
[`syntax-decisions.md`](syntax-decisions.md) and removed here. No "recently
ratified" section, no decided history — decided decisions never reappear.

**House rule for whoever edits this file (enforced — a card missing any of these is
not ballot-ready; Tower v2 Focus Mode renders these as labeled facets, so use the
exact bold labels):** every full decision card carries `**Gist:**` (one VERY short
plain sentence — the headline), `**Story.**` (a real person with an
American-traditional name and what they're doing), `**In the wild:**` (a fenced
```jet block of realistic project code where this bites), `**Other languages:**`
(short fenced blocks for Rust/TS/Swift/etc. when a cross-language compare helps),
`**Tradeoffs:**` (a compact table, one row per option, columns that actually differ —
subagent-reviewed), and a **worked example of every option** (each
`- **Option X — <name>.**` bullet with its own fenced ```jet/```shell block; mark the
recommended one `(recommended)`). Close with `**Recommendation:**` + a one-line why.
Put Owner Q&A in `**Owner Q …**` blocks — Tower routes those to a separate Q&A facet,
so keep them out of the recommendation. Decisions not yet drafted to that bar are
listed below as one-liners with a recommendation; expand one into a full card when
it's time to decide it.

---

## Open decisions

---

### D-COMPILERSEAMS1 — Workspace crate graph for the compiler seam split (c160 step 3b)

**Gist:** Choose how the seven compiler seams become Cargo workspace crates.

**Story.** Tyler is porting the Jet compiler to self-host. He pulls in the `sema` crate as a library dependency for his new IR pass. He wants to import only the types he needs without pulling in the lexer or codegen — and the `cargo tree` output should make that dependency graph obvious, not a monolith.

**In the wild:**
```jet
// Tyler's self-host pass
use jet_sema::{CheckedBundle, Type}   // wants ONLY sema + its deps, nothing else
use jet_tir::{TFunc, TExpr}           // wants the typed IR for his IR pass
// he must NOT have to pull in jet_codegen just to read a TExpr
```

**Other languages:**
```
Rust (rustc):       rustc_lexer / rustc_parse / rustc_middle (shared types + TIR)
                    / rustc_codegen_ssa / rustc_driver — shared types ARE in rustc_middle,
                    not a separate foundation crate
TypeScript (tsc):   single package; no crate split
Swift (swiftc):     single library; seams are internal modules, not separate packages
Roslyn (C#):        Microsoft.CodeAnalysis (shared core) + Microsoft.CodeAnalysis.CSharp
                    — explicit foundation crate pattern
```

**Background — the dependency reality (verified by orientation analysis):**
- Syntax.rs: zero internal deps (pure leaf)
- Diagnostics.rs: depends on Syntax only
- AST.rs: depends on Diagnostics + Syntax
- Lexer: depends on Diagnostics + Syntax
- Parser: depends on AST + Diagnostics + Lexer + Syntax + Generics
- Sema: depends on AST + Diagnostics + Generics + Loader + Publish + Syntax + Traits
- TIR (Source/Codegen/TIR/): BOTH imports from Codegen (the `Cx` context type, `mangle`, `rust_param_type`) AND is imported by Codegen (TFunc/TExpr types used in Items.rs). Bidirectional dependency — cannot be two Cargo crates without resolving this.
- Codegen: depends on AST + FFI + Sema::CompileMode + Syntax + TIR
- Comptime: depends on AST + Diagnostics
- Driver: depends on everything via composition

**Tradeoffs:**

| Option | Crate count | TIR+Codegen | Shared-type home | `cargo tree` |
|--------|-------------|-------------|------------------|--------------|
| A — Foundation + split seams | 9 (1 foundation + 7 seams + 1 bin) | Two seams — Cx moves into TIR so codegen can dep on TIR one-way | `jet-foundation` crate (Syntax+Diagnostics+AST) | Clean layered graph |
| B — Foundation + merged codegen | 8 (1 foundation + 6 seams + 1 bin) | One crate `jet-codegen` (TIR as internal submod) | Same `jet-foundation` | Clean; TIR/codegen not separately importable |
| C — No foundation; shared types stay in jet | 7 seams depend on the root `jet` lib | Merged; no cycle problem | `jet` (current root) holds Syntax/Diag/AST | Seams are pure consumers; root stays fat |

**Worked examples:**

- **Option A — Foundation crate + full seam split.**
  Crate graph:
  ```
  jet-foundation  (Syntax, Diagnostics, AST, Span, Generics)
  jet-lexer       → jet-foundation
  jet-parser      → jet-foundation, jet-lexer
  jet-sema        → jet-foundation, jet-parser, jet-traits, jet-loader
  jet-tir         → jet-foundation, jet-sema          ← Cx moved into jet-tir
  jet-codegen     → jet-foundation, jet-sema, jet-tir  ← one-way dep
  jet-comptime    → jet-foundation, jet-sema
  jet-driver      → all seam crates
  jet (bin)       → jet-driver
  ```
  Breaking the Cx cycle: move `struct Cx` and its `rust_param_type`/`mangle` helpers from Codegen/Context.rs into TIR/mod.rs. Codegen then depends on jet-tir for Cx, not the other way. This is a mechanical move (~200 lines).

- **Option B — Foundation crate + merged codegen seam. (recommended)**
  Crate graph:
  ```
  jet-foundation  (Syntax, Diagnostics, AST, Span, Generics)
  jet-lexer       → jet-foundation
  jet-parser      → jet-foundation, jet-lexer
  jet-sema        → jet-foundation, jet-parser, jet-traits, jet-loader
  jet-codegen     → jet-foundation, jet-sema     ← TIR is a submod inside; no cycle
  jet-comptime    → jet-foundation, jet-sema
  jet-driver      → all seam crates
  jet (bin)       → jet-driver
  ```
  TIR stays as `jet_codegen::TIR::*`. The self-host port imports `jet-codegen` to get TIR types. Fewer crates, no cycle surgery, same I6 machine-check benefit. LSP can still import `jet-codegen::TIR::TFunc` to inspect the typed IR.

- **Option C — No foundation; shared types stay in the root jet crate.**
  Crate graph:
  ```
  jet (lib)       (ALL of today's Source/ — Syntax, Diag, AST, Lexer, Parser, Sema, Codegen, Comptime, Driver, Loader, etc.)
  jet-jit         → jet
  jet-net         → (no dep on jet lib)
  jet (bin)       → jet (lib), jet-jit, jet-net
  ```
  This is essentially the current structure with jet-net and jet-jit added as workspace members. The "seams" remain internal modules in one library crate. No cycle problems. Least churn. `cargo tree` doesn't show seam boundaries, only the external dep structure.

**Recommendation:** B — foundation crate + merged jet-codegen seam. Avoids the ~200-line Cx surgery of A while still getting the crate-by-crate boundary the self-host port and I6 machine-check need. Tyler can import `jet-codegen::TIR::TFunc` cleanly.

**Owner Q1:** Option A moves Cx into jet-tir (mechanical ~200-line refactor). Option B keeps TIR+codegen merged. Which seam boundary matters more to you — strict TIR/codegen separation (A) or fewer crates with less surgery (B)?

**Owner Q2:** Crate names — do you prefer the `jet-<seam>` convention (jet-lexer, jet-parser, jet-sema, jet-codegen, jet-comptime, jet-driver, jet-foundation) or a different naming scheme? Aviation/launch theme alternatives: `runway` (lexer), `flightplan` (parser), `airspace` (sema), `payload` (codegen), `groundcrew` (driver), or just the plain technical names?

---

### D-CTFIND1 — What does `find` mean in comptime Tier 1?

**Gist:** Decide whether `find` in the Tier-1 effect list means hashing the existing U4 import-discovery directive or a new comptime glob builtin.

**Story.** Priya is building a compile-time asset pipeline. She wants the build to be byte-reproducible — if the set of `.png` files in `assets/` changes, the build must re-run and the hash in `.jet/lock` must update. She writes `comptime ASSET_LIST = find("assets/**/*.png")` expecting it to list files.

**In the wild:**
```jet
// What Priya writes — does this work?
comptime ASSETS @= find("assets/**/*.png")   // list all PNG paths at build time

// What currently exists — U4 import discovery (different layer):
imports: find("./modules")   // auto-discovers .jet modules in the loader
```

**Other languages:**
```
Zig:        comptime — @import resolves at comptime; no glob builtin
Rust:       build.rs glob via walkdir/glob crates; not in-language
Jai:        #run { files := File.find("**/*.png") } — runtime code at comptime
D:          import(file) embeds a single file; no glob
```

**Tradeoffs:**

| Option | What `find` means | Implementation layer | Impact on .jet/lock |
|--------|-------------------|----------------------|---------------------|
| A — U4 import discovery | `imports: find("./path")` directive; hash the set of discovered .jet module paths | Loader layer, already built | Hash sorted discovered paths + their tree hash into `[[comptime_inputs]]` |
| B — New comptime glob builtin | A new `find(glob) -> [String]` comptime builtin; evaluates in the comptime interpreter | Comptime layer, new work; glob matching (std has limited glob; likely needs `glob` crate) | Hash the result list into `[[comptime_inputs]]` |

**Worked examples:**

- **Option A — `find` = U4 import discovery.**
  ```jet
  // This is what find already does — auto-discover modules:
  imports: find("./services")   // finds services/auth.jet, services/billing.jet, etc.
  
  // Tier-1 addition: when the loader runs find("./services"), it records the
  // discovered file set hash into .jet/lock:
  //   [[comptime_inputs]]
  //   path = "find:./services"
  //   hash = "sha256:abc123..."
  //
  // If auth.jet is added/removed, .jet/lock hash changes → --locked CI fails → rebuild.
  ```
  No new syntax. No new builtin. Just hash-recording of the existing U4 find directive.

- **Option B — New comptime glob builtin. (recommended)**
  ```jet
  // New comptime builtin for general file discovery:
  comptime ASSETS @= find("assets/**/*.png")
  // → ["assets/logo.png", "assets/icons/star.png", ...]
  // Hashed into .jet/lock; change the files → rebuild.
  
  // ALSO adds Tier-1 hash-recording to U4 imports:find("./path") (orthogonal)
  
  comptime SHADERS @= find("shaders/*.glsl")
  comptime SHADER_COUNT @= SHADERS.len()  // 12
  ```
  Priya's asset pipeline works. Also useful for codegen from schemas, i18n, etc. Requires a glob implementation (std path matching is limited; `glob` crate or a minimal hand-rolled pattern matcher).

**Recommendation:** B — a real `find(glob) -> [String]` comptime builtin is the more useful Tier-1 primitive. U4 import discovery gets hash-recording as a side effect but doesn't expose `find` as a user-callable expression. Option A means `find` is not callable in `comptime` blocks — just a manifest directive — which makes the "Tier-1 effect" framing awkward.

**Owner Q1:** Should `find` in Tier 1 be a comptime callable that returns a list of paths (B), or is it only the U4 import-discovery directive that gets its results hashed (A)? This determines whether Priya's `comptime ASSETS @= find("assets/**/*.png")` is valid Jet.

**Owner Q2 (if B):** Is a minimal hand-rolled glob OK for v1 (supports `*`, `**`, `?`), or should we use the `glob` crate as a bootstrap dep (same posture as ureq/rusqlite — owner-gated, I6 holds, native-ize obligation)?

---

### D-HTTPLIB1 — `jet.http` API surface: client + server shape

**Gist:** Decide the API shape for `jet.http` — the full HTTP client+server library that replaces Go's `net/http` as the owner-expanded mandate from D-NETDEP1.

**Story.** Marcus is building a REST microservice in Jet. He needs to make outbound API calls (GET/POST with JSON bodies, auth headers, timeouts) and also serve HTTP endpoints — a `GET /users/:id` route that returns JSON. He wants the code to be obvious to a Go or Python developer who's never seen Jet before.

**In the wild:**
```jet
// What Marcus wants to write — server side:
http.serve("0.0.0.0:8080", fn(req) {
    match req.path {
        "/users/{id}" => respond(.{ status: 200, body: get_user(id) })
        _             => respond(.{ status: 404 })
    }
})

// Client side:
resp @= http.get("https://api.example.com/users/42")
    .header("Authorization", "Bearer {token}")
    .send()?
user @= json.decode<User>(resp.body)?
```

**Other languages:**
```go
// Go net/http — function-based, mux-routed:
http.HandleFunc("/users/{id}", func(w ResponseWriter, r *Request) {
    json.NewEncoder(w).Encode(getUser(r.PathValue("id")))
})
http.ListenAndServe(":8080", nil)
// Client:
resp, _ := http.Get("https://api.example.com/users/42")
```
```python
# Flask — decorator-routed:
@app.route("/users/<id>")
def get_user(id): return jsonify(user)
# httpx client:
r = httpx.get("https://api.example.com/users/42")
```
```rust
// axum + reqwest:
Router::new().route("/users/:id", get(|Path(id): Path<String>| async { Json(user) }))
// reqwest client: reqwest::get(url).await?.json::<User>().await?
```

**Tradeoffs:**

| Option | Server model | Client model | Routing | Learning curve |
|--------|-------------|-------------|---------|----------------|
| A — Function-first (Go-style) | `http.serve(addr, handler_fn)` + `http.mux()` router | `http.get(url).send()` builder | Pattern matching in handler or mux | Lowest; familiar to Go/JS devs |
| B — Handler objects (Axum-style) | Router with typed extractors; strongly typed params | Same request builder | `Router.get("/path", handler)` | Medium; more type-safe, more Jet-idiomatic |
| C — Unified request/response (Rack/WSGI) | Single `fn(Request) -> Response` everywhere; no magic | Same | User-level routing in the fn | Lowest floor; least opinionated |

**Worked examples:**

- **Option A — Function-first, Go-style. (recommended)**
  ```jet
  use jet.http as http
  use jet.json as json
  
  fn main() {
      mux @= http.mux()
      mux.get("/users/{id}", fn(req) {
          id @= req.params["id"]
          user @= db.get_user(id) else return http.not_found()
          http.ok(json.encode(user))
      })
      http.serve("0.0.0.0:8080", mux)?
  }
  
  // Client:
  resp @= http.get("https://api.example.com/data")
      .header("Accept", "application/json")
      .timeout(5000)
      .send()?
  ```

- **Option B — Typed extractors, Axum-style.**
  ```jet
  use jet.http as http
  
  fn get_user(id: Path<String>, db: State<Db>) -> Response {
      user @= db.get(id.value) else return http.Response.not_found()
      http.Response.json(user)
  }
  
  fn main() {
      router @= http.Router.new()
          .get("/users/{id}", get_user)
      http.serve("0.0.0.0:8080", router)?
  }
  ```

- **Option C — Rack/WSGI unified fn.**
  ```jet
  use jet.http as http
  
  fn handler(req: http.Request) -> http.Response {
      match req.method, req.path {
          "GET", "/users/{id}" => {
              user @= db.get(id) else return .{ status: 404 }
              .{ status: 200, body: json.encode(user) }
          }
          _ => .{ status: 404 }
      }
  }
  
  fn main() { http.serve("0.0.0.0:8080", handler)? }
  ```

**Recommendation:** A — function-first with a mux router. Lowest barrier for Go/Python devs Marcus is coming from; router is explicit and testable; strongly typed paths can be layered on later. The request builder client API is natural Jet (chained methods, `?` propagation).

**Owner Q1:** Which server model — function-first mux (A), typed extractors (B), or unified handler fn (C)?

**Owner Q2:** Should `jet.http` be a single module with both client and server, or two: `jet.http.client` and `jet.http.server`? Go uses one `net/http`; most modern frameworks split them.

**Owner Q3:** What's the v1 scope? Minimum viable: (a) HTTP/1.1 only (ureq bootstrap), (b) HTTP/1.1 + HTTP/2, (c) HTTP/1.1 + HTTP/2 + WebSocket. The bootstrap crate covers (a); native implementation expands later.

**Owner Q4:** TLS — for the client, should HTTPS work out of the box (requires bundling a TLS stack or calling into system TLS), or is the v1 scope HTTP-only with HTTPS as a follow-on?

---

## Recently ratified — context (no action)

_**D-NETDEP1** (ratified 2026-06-26): **A** — approve a small pure-Rust HTTP crate
(`ureq`/`minreq`, runtime-side, owner-gated, I6 holds) to back D-CTEFFECT1's build-time
`fetch(url, sha256:)`. **Owner expanded the mandate:** the goal is a full, complete HTTP
library — client **and** server, better than Go's `net/http` — as a Jet core library; the
crate is the bootstrap, the native-ize end-state is a first-party Jet HTTP stdlib. c157's
`fetch` backend is now unblocked and ships first; the client+server API surface becomes its
own core-library track with its own design + ballots before that code is written._

_Earlier batch (ratified 2026-06-25, second pass): **D-DOTCTOR2** (A — retire the
dotless `T { }`; `T.{ … }` is the sole named-construction spelling, E0320) ·
**D-METAREFLECT1** (B — one reflected `T.reflect()` handle) · **D-PLUGIN1** (B —
`target: plugin` = sandboxed WASM, safe-by-default, WASM-runtime dep owner-gated) ·
**D-WORKSPACE2** (A — `workspace` keyword / `workspace.jet`, kept the industry term
over the aviation menu) · **D-METADERIVE1** (A — `derive Trait for T` + source-fragment
re-entry; errors pin at the `#[…]` trigger, matches Rust/Swift macros) · **D-DEP-WASM1**
(A — wasmtime + Component Model backs the D-PLUGIN1 sandbox; reuses the already-approved
Cranelift, runtime-side only so I6 holds). Tracking cards: c81, c155, c156, c158._

_Prior batch (ratified 2026-06-25): **D-CTMARKER1** (C — `$` for the comptime
splice site only + a `comptime { … }` execution block) · **D-WORKSPACE1** (B — fully
computable `workspace.jet` index) · **D-METADEPTH1** (A — reflection/derives only;
full Jai → frozen c154) · **D-CTEFFECT1**, **D-DOTCTOR1**, **D-MONOREF1**,
**D-BUILDPROFILE1**, **D-CTCODEGEN1**, **D-COMPILERLIB1** · plus **D-ENC-DYN1** (A+)
and **D-ENC-YAML1** (A) — build c152, shipped. Tracking cards: c154–c161._


_Background: **D-ASSOC-NOW** was decided **C** (fund both streams: complete
associated types → c149/c72 layer 2, and D-PARSE-1 → c111) and recorded in
[`syntax-decisions.md`](syntax-decisions.md)._

---

**Still deferred (not blocking; expand to a card when needed):**
- **D-SERDE-ACCESS — dynamic-tree accessor API.** How a user reads an untyped
  `Json`/agnostic `DataTree` by hand: pattern-match (shipped today) vs a fluent accessor
  (`tree.field("x").int()?`, `.text()`, `.bool()`, indexing). Only matters for the
  hand-impl / dynamic path (D-SERDE2), not the typed derive. Recommend: keep
  pattern-match as the floor; add minimal fluent accessors if hand-impl ergonomics demand it.

---

> **Drained 2026-06-24 (batch 5).** Owner decided the last open cards: **D-EFF4 = B**
> (ship the closed ten effects now — Net/Fs/Io/Db/Time/Rand/Env/Exec/Log/Gpu — and reserve a
> future `effect <Name>` user-declaration form), **D-EFF5 = A** (flat effect lattice; `#(Io)`
> = console only, no umbrella; `Io`→`Console` rename left as optional polish), and
> **D-JITDEP1 = approve Cranelift** for JIT tier-1 (runtime-side only, I6 holds; the own
> bytecode-VM and own native-JIT progression are frozen board cards so they're not lost).
> All recorded in `syntax-decisions.md`; the effect-system cluster (c62) is now unblocked.

> **Drained 2026-06-24 (batch 4).** The owner ratified all 11 remaining open full cards:
> **D-SIMD2 = A** (method-reduce SIMD surface; operator overloading on built-in lane types
> only), **D-SERDE2 = A** (Swift-plain hand-impl: `encode`/`decode`, `DataTree`, `DecodeError`),
> **D-SERDE3 = C** (typed `RenameAll` menu camel/snake/pascal/kebab/screaming),
> **D-SERDE4 = B, owner-modified** (umbrella `#[Codable]`; one-way `#[Encode]`/`#[Decode]`),
> **D-SERDE5 = A** (per-field bracket markers `#[Rename]`/`#[Skip]`/`#[Default(expr)?]`/`#[Flatten]`,
> absent-optional omitted, struct-flatten now), **D-SERDE6 = C** (typed `decode<T>` turbofish +
> expected-type; turbofish blessed as general grammar), **D-SERDE7 = A + ship chooser now**
> (externally tagged default; `#[Tag("type")]`/`#[Untagged]` container chooser — distinct from
> D-SERDE5 field attrs), **D-SERDE8 = A** (lenient default + `#[DenyUnknownFields]`),
> **D-NOSTD1 = A** (platform-implied std opt-out), **D-IF3 = A** (`if x == { … }` required
> dispatch marker; E0992/E0993), **D-FMT1 = A** (author-intent single-line bodies). The two
> **clarification corrections** were confirmed: **C-CASING** (plan tags → D-CASING1 PascalCase)
> and **C-MANIFEST** (`pkg.jet` → `pack.jet`). All recorded in `syntax-decisions.md`, cards
> stripped. Serde increment-2 implementation unblocked end-to-end (sidequests/serde-model.md).


> **Drained 2026-06-24 (batch 3).** Two follow-on cards ratified: **D-JSONVERB1 = A**
> (`json.to_string(v)` + `json.to_string_pretty(v)`, 2-space indent — renames/retires
> `json.render`; keeps Jet's one `to_`-prefixed conversion idiom, matching ratified `to_float`
> S42; bare `json.string`/`json.stringify` rejected) and **D-TXN4 = A** (`#Transact(order) { …
> order.on_commit(…) }` — the scope's name *is* the handle, mirroring ratified `region r { …
> r.alloc(…) }`; refines D-TXN3's `scope.on_commit` → `<name>.on_commit`, semantics unchanged;
> the D-TXN2 fix-it string is updated to match). The `.Type()`-conversion idea (`x.Float()`)
> was discussed and **declined** — `x.to_float()` (S42) stays as ratified and shipping; no
> reopen. Recorded in `syntax-decisions.md`, cards stripped.

---

> **Drained 2026-06-24 (batch 2).** The owner ratified six cards from the missing-decision
> audit: **D-DBG3 = A** (`jet debug` interactive surface — `step`/`next`/`continue`/`finish`
> + `s`/`n`/`c`/`f` aliases, `(jet)` prompt, `<- here`/`locals:` layout); **D-LINALG1 = A**
> (`jet.linalg` names `Vec2/3/4`/`Mat3/4`, `.dot`/`.cross`/`.matmul` — A names as aliases over
> a `Vec<N>`/`Matrix<M,N>` generic substrate, per owner); **D-SUPPLY1 = A** (dedicated
> `jet vendor` / `jet audit` verbs + `--vendor-dir`, SBOM as a `--sbom` flag); **D-TXN3 = A**
> (`scope.on_commit(() => {…})` library form, no new keyword — the D-TXN2 fix-it string is
> updated to match; the "name the transact scope" follow-on is now open as **D-TXN4**);
> **D-NUMOPS2 = A** (sized/unsigned integers inherit the D-NUMOPS1 trap-on-overflow default;
> `wrapping(…)` is the opt-in); **D-QUAL3 = C** (a `#UnitFamily` mints one distinct type per
> member — `usd`→`Usd` — so signatures read `price: Usd`; the family tag is PascalCase
> `#UnitFamily`). All recorded in `syntax-decisions.md`, cards stripped, plans unblocked
> (dap-debugger, math-linalg, package-ecosystem-trust, transact-rollback, dsg9, units; c68
> unblocked by D-QUAL3).

---

> **Drained 2026-06-24.** The owner ratified the last two open cards: **D-BENCH1 = A**
> (`#Bench "name" { … }` region-benchmark block, sibling of `#Test`, run by the existing
> `jet bench` verb) and **D-PKGSIGN1 = B + A opt-in** (SHA-256 checksum is the always-on
> integrity floor; Ed25519 author signing is an opt-in, non-blocking layer — `require_signed`
> off by default). Both recorded in `syntax-decisions.md`, cards stripped, plans unblocked
> (epoch-3/testing-docs-ergonomics.md §4; sidequests/package-ecosystem-trust.md §4).

---

> **Memory-model gate CLOSED — ratified 2026-06-23.** The owner decided all three gate
> cards: **D-CAP8 = C** (infer in bodies, freeze at `api: explicit`), **D-CAP9 = D** (`*x`
> = raw-of, dereference becomes postfix `p.*`, `*T` replaces `Ptr<T>`), **D-CAP10 = A**
> (overloads out of scope; call-site-sigil disambiguation on a single definition). Recorded
> in `syntax-decisions.md`; cards stripped. The whole access-capability model
> (`docs/prompt-memory-model-final.md`) is now unblocked — see
> `docs/research/memory-model-implementation-plan.md` for the build order.

---

> **Drained 2026-06-22.** The owner's 2026-06-22 batch ratified every open full card —
> D-UNSAFE2, D-FIXARR1, D-CAP2/3, D-EFF2/3, D-MIGRATE2A/B/C/D/E/F, D-JSONOUT1, D-ARGS1,
> D-MATHLIB1, D-SIMD1, D-REACT1, D-FANOUT2, D-STRPARSE1, D-CTCORE1, D-JIT1, D-HOTSWAP1,
> D-DEVMODE1, D-SOA2A/B/C/D, D-TEST1, D-TEST4, D-BIND2, D-NUMOPS1, D-SERDE1, D-ITER1 (plus
> the earlier batch D-EFF1/D-QUAL1/D-TXN1/D-MIGRATE1/D-SOA1 and D-DBG2). All are recorded
> in `syntax-decisions.md` and their cards stripped from this file. The effect-system
> surface is now fully decided (D-EFF1+D-QUAL1+D-EFF2+D-EFF3). **D-MUTSELF1** (self-mutation
> in `mut self` methods) was opened and ratified 2026-06-23 (option A) — recorded in
> `syntax-decisions.md`, card stripped. The memory-model gate (D-CAP8/9/10) was opened and
> ratified 2026-06-23 — see the note above. **No full decision cards remain open.** What's left
> below is informational only: the **deferred-ballots list**
> (stubs to promote when their prerequisites land), the **B6 `defer`** note, and the
> **Coverage / D-COV1** tooling note. Cards **c25** (range sugar) and **c55** (REPL v2) are
> implement-only. Submitting a decision records it in `syntax-decisions.md` and removes it
> from this file.

---

## Deferred ballots — promote when reached

The items below are not ready for owner decision. Each has a real user story
and a clear reason to wait. Promote a stub to a full card when its
prerequisite is ratified or its milestone is reached.

---

**D-PUBLISH1 — `jet publish` command shape + semver/resolver policy (board card c96).**
*User story:* Saoirse cuts a release of her Jet library and Amara pins a semver range to it.
*Decision (when promoted):* the `jet publish` command surface, version-immutability /
re-publish-refusal policy, and the resolver default (highest-compatible vs exact pins +
explicit update; lockfile default). *Why deferred:* rides **c50** (build-from-source) and
**c56** (registry upload) infra, both unverified/soft-blocked on dep approvals. Promote to a
full card with worked `jet publish` shell examples once M12.2 infra is verified.
Rec direction: `jet publish` infers version from `pkg.jet`, refuses re-publish + a dirty
tree, resolver defaults to highest-compatible with a committed lockfile. From the 2026-06-20
persona run (Saoirse, Amara).

---

**D-JITDEP1 — DECIDED 2026-06-24: approve Cranelift** (runtime-side JIT tier-1, I6 holds).
Recorded in `syntax-decisions.md`. Active work = board card for the Cranelift backend over
the `JitBackend` seam; the own-bytecode-VM and own-native-JIT progression are frozen cards.

---

**D-QUAL4 — Plain marker-tag type-position spelling (prefix vs postfix).**
*User story:* A web dev marks a value `#Tainted` at its source and needs to write
the *type* of a tainted string in a function signature — `flagged: #Tainted String`
vs `String #Tainted`. Same question for `#SingleUse`, `#NoCopy`, and the typestate
markers — the plain (non-parameterized) value-tags that attach to an existing type
rather than minting a new one (so D-QUAL3's "mint a type" Option C doesn't apply).
*Decision (when promoted):* prefix `#Tag Type` (matches every other Jet `#Marker`:
`#Test fn`, `#Numeric distinct`) vs postfix `Type #Tag`. Rec direction: **prefix**, for
one consistent marker idiom. *Why deferred:* no ready consumer — units (c68) ride D-QUAL3
and mint types; the first plain value-tag consumer is taint (D-TAINT1, gated on D-EFF1)
or single-use (D-LIN1, c71). Promote to a full card when c71 or the taint work starts.
Split from D-QUAL3 on 2026-06-24 (a single card can't pick both axes).

---

**D-PROP1 — Effect prohibitions: implicit propagation of `#(no_…)`.**
*User story:* A security engineer wants to know, by reading the root call
site, that a call graph never touches the network — without auditing every
callee. He writes `#(no_net)` on a function and the compiler traces every
reachable call for a net effect, naming the violating path.
*Why deferred:* Rides **D-EFF1** (the effect-propagation engine itself) plus
D-QUAL1's surface (`#(…)`); prohibition is the inverse-lattice follow-on once
positive effects propagate. Sequencing: D-EFF1 → D-PROP1. Board items #24/#4.

---

**D-ROLE1 — Time-varying roles: typestate + time.**
*User story:* A hotel booking system dev wants to express that a `Reservation`
is `#pending` before payment and `#confirmed` after — and that calling
`check_in` on a `#pending` reservation is a compile error.
*Why deferred:* Requires the typestate machinery from **D-STATE1** (gated on
D-QUAL2) to be ratified first; "time-varying" adds a temporal ordering
constraint on top of static typestate, a separate design question. Board item #13.

---

**D-REFINE1 — Refinement types.**
*User story:* A numeric processing library author wants `PositiveInt` to be a
type the compiler can prove is always > 0, so she doesn't pepper every
function with `require(n > 0)`.
*Why deferred:* Refinement types require a proof/SMT layer that is not in the
roadmap for v1; the simplicity ratchet (I8) requires a concrete milestone slot
and owner sign-off before any work begins. Board item #19.

---

**D-BUDGET1 — Budgets as types.**
*User story:* A systems developer writing a real-time renderer wants to express
that `render_frame` has a 16ms CPU budget and have the compiler warn if a
called function is known to exceed it.
*Why deferred:* Requires comptime cost-bound inference, which is not in the
v1 roadmap; no prior-art consensus on how to make it ergonomic without macros
(I8 / no macros). Board item #22.

---

**D-IFC1 — Information-flow and compliance tracking.**
*User story:* A fintech dev wants to annotate a value as `#pii` (personally
identifiable information) and have the compiler refuse to let it flow into a
logging call or a non-encrypted storage write without an explicit sanitize
step — enforced at compile time, not by code review.
*Why deferred:* This is **D-TAINT1 Option B** (full information-flow control —
security-label lattice, principals, `declassify`), which the **owner explicitly
deferred to post-Epoch-3 on 2026-06-21** when ratifying D-TAINT1 Option A
(`#tainted` + sanitizers). Captured here so it is not lost. Generalizes D-TAINT1
and requires the full effect/tag propagation model from D-EFF1 and D-QUAL1 to be
ratified first; the compliance dimension (what counts as a legal sink) is a policy
question that also interacts with the manifest capability model (D-QUAL1 Option A,
manifest surface). Board items #30/#33.

---

**D-REPLAY1 — Opt-in record and replay.**
*User story:* A game developer wants to record a session's inputs, replay
them deterministically to reproduce a bug, and have the compiler ensure no
hidden state (system clock, random, I/O) is read during replay without being
mocked.
*Why deferred:* Requires the effect system (D-EFF1) to tag non-deterministic
effects and a runtime record/replay harness; neither is in the v1 roadmap.
Board item #7.

---

**D-REVERSE1 — Opt-in reversible computation and solver integration.**
*User story:* A constraint-based UI layout author wants to write the forward
constraint (`width = parent.width - padding * 2`) and have Jet automatically
solve for `padding` given a target `width` — without writing the inverse by
hand.
*Why deferred:* Requires a reversibility annotation on functions and a
solver/SMT backend; no prior-art consensus on making this ergonomic without
macros or dependent types. Board item #36.

---

**D-PROTO1 — Protocol and session type generation.**
*User story:* A network protocol implementer wants to declare a
request/response handshake sequence as a type and have the compiler generate
both the client and server stubs, rejecting code that sends messages out of
order.
*Why deferred:* Session types require linear types (used exactly once, in
order) and typestate; **D-LIN1** (linear tag) and **D-STATE1** (typestate),
both gated on D-QUAL2, are prerequisites, and the code-generation surface for
protocol stubs is a separate design. Board item #9.

---

**D-VERIFY1 — Formal verification and proof integration.**
*User story:* A cryptography library author wants to attach a machine-checked
proof that her `constant_time_eq` function runs in time independent of its
inputs, and have the Jet toolchain refuse to ship the library if the proof
doesn't hold.
*Why deferred:* Requires a proof-carrying-code or SMT integration layer that
is explicitly post-v1; the simplicity ratchet (I8) bars this without a
concrete roadmap slot and owner sign-off. Board items #15/#17.

---

## B6 `defer` — already decided, no ballot

`defer` is solved; nothing to vote on. **D-DEFER1 (ratified + implemented 2026-06-20)** shipped `core.scope.guard(() => {…})` — a stdlib value whose `Drop` runs the stored lambda LIFO on every exit path including `?`. `defer`-as-primary stays rejected (S63); the `defer` keyword stays declined (D-SUGAR5).

```jet
use core.scope

fn copy_file(src: String, dst: String) -> () ? Error {
    f :: core.fs.open(src)?
    g1 :: scope.guard(() => { core.fs.close(f) })   // replaces `defer close(f)`
    g :: core.fs.create(dst)?
    g2 :: scope.guard(() => { core.fs.close(g) })   // fires before g1, even on early return
    core.fs.copy(f, g)?
}
```

**Reopen (owner-only):** you could later add `defer expr` as sugar over `scope.guard` (same Drop-backed lowering, zero runtime cost). For: it's the spelling Jai/Go/Swift/Odin/Zig converge on. Against: D-SUGAR5 declined it; it adds a second cleanup spelling and reintroduces Go's leak-by-omission class. No agent reopens this without your instruction.

---

## Coverage — D-COV1 (deferred, no ballot needed)

The epoch-3 plan scopes coverage as "tooling only — no new syntax; couples to the
test runner in `Source/main.rs` (`run_test`)." There is no user-facing surface
decision: `jet test --coverage` is the spelled-out verb and the output format (LCOV
/ HTML / stdout summary) is an implementation choice, not a syntax choice.

**Prior art:**
- **Rust tarpaulin** — `cargo tarpaulin --out Html`; produces HTML + lcov. No new
  Rust syntax. Jet takeaway: a `--coverage` flag on `jet test` is the right shape.
- **llvm-cov / cargo llvm-cov** — output: `--json`, `--lcov`, `--html`, `--text`.
  Jet takeaway: multiple formats are useful but can be deferred to a `--format`
  flag.
- **Python coverage.py** — `coverage run`; then `coverage report` / `coverage html`.
  Two-step. Jet takeaway: a single `jet test --coverage` that prints a summary to
  stdout (and optionally writes a report) is simpler than a two-step model.

**Deferred note:** if coverage ever needs a source annotation (e.g. `// @no_cover`
to exclude a line from the report), that is a syntax decision requiring a ballot.
Until then, coverage is tooling-only and can land without owner ratification. The
implementation milestone (exit criterion: `jet test --coverage` reports per-line /
per-function coverage) can proceed independently of D-TEST1 and D-TEST4.

---

