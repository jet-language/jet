# Decision ballots (owner's queue)

Open decisions awaiting the owner. **Ratified choices live only in
docs/spec/syntax-decisions.md** (and, for milestone-scoped IDs, in the relevant
plan under docs/plans/) — when the owner decides, agents add the row there and
remove it from this file. This file is the *pending queue only*; it never
duplicates ratified content.

Decide one group at a time. A group must be fully decided before its milestone
starts (plans in docs/plans/ are blocked on these IDs).

## How to read this file

- **Strategic (Part 1)** — product direction for all of Epoch 2. These set the
  bar that every milestone plan is measured against. Decide these first; they
  are cheap to answer and expensive to get wrong.
- **Milestone gates (Part 2)** — the few CEO-level calls each detailed plan
  needs before its agent starts. One compact table per milestone, each with a
  recommendation and a *default if deferred* so work is never fully blocked.
- **Feature ballots (Part 3)** — concrete syntax/semantics distilled from the
  research folder (`docs/research/*`) and `docs/plans/owner-todo.md`. Each option
  carries a worked example (Jet code, terminal output, or error text) so the
  choice can be made from real use, not abstractions.
- Every ID here is **pending**. Ratified items are listed only by reference at
  the bottom. A `**Rec**` is the agents' recommendation, not a decision — agents
  must not substitute a different option if you pick one.

Glossary: *epoch* = a large human-facing era (Epoch 1 = language core, Epoch 2 =
production platform). *edition* = a per-project compatibility marker that lets
old code keep compiling when syntax changes (Rust-style). *tier-2* = post-v1
reference features (`view`/`ref`) for experts. *ring* = the first-party
`jet.*` package set that ships beside the compiler.

---

# Part 1 — Epoch 2 product direction (CEO ballot)

These are whole-epoch choices. Agents should not finalize a milestone's scope
until the strategy it depends on is set. Full narrative context lives in
docs/plans/epoch-2/README.md; this is the decision surface.

## Group E2V — Strategic vision

| ID | Question | Options (one line each) | Rec |
|---|---|---|---|
| E2-V1 | Who is Epoch 2 GA *for*? | A: beginners + small-tool authors · B: small teams first · C: enterprise platform buyers first | **A** (with B as the proof) |
| E2-V2 | What does "production platform" mean at GA? | A: credible for internal services + CLIs · B: also public-facing SaaS · C: also regulated/audit-heavy | **A** |
| E2-V3 | Who must we beat convincingly? | A: Python/Node scripts · B: Go services · C: Rust small tools · D: Zig/C systems | **A primary, B secondary** |
| E2-V4 | How sacred is single-file `jet run`? | A: forever default path · B: package-first for new users after tutorial · C: workspace-first for teams | **A** |
| E2-V5 | Concurrency model lock for the epoch | A: tasks/channels only (S53) · B: reserve async syntax for Epoch 3 w/ public note · C: promote async inside Epoch 2 | **A** |
| E2-V6 | Expert/low-level appetite | A: smoke demos only · B: credible for systems programmers, still gated · C: defer C FFI + freestanding to Epoch 3 | **B** |
| E2-V7 | Networking/services ambition | A: internal HTTP/CLI services only · B: small public APIs with TLS · C: defer services to Epoch 3 | **B** |
| E2-V8 | Supply-chain minimum bar | A: pub.dev-class (semver + lockfile) · B: enterprise-class (vendor, audit, SBOM, mirror) · C: air-gapped-first | **B** |
| E2-V9 | Editor ecosystem priority | A: VS Code/Cursor + Zed dev extension · B: VS Code/Cursor only until GA · C: also Neovim in Epoch 2 | **A** |
| E2-V10 | Public launch trigger | A: E2-M17 technical GA · B: separate launch milestone after audits · C: no encoded epoch ever | **B** |
| E2-V11 | Governance at launch | A: OSS project, owner-led LTS · B: foundation prep in Epoch 2 · C: defer all governance messaging | **A** |
| E2-V12 | JetOS / pure eval / layer-3 boundary | A: `pure fn` + `jet eval --pure` only (S60) · B: package recipes in Epoch 2 · C: JetOS research-only | **C** |

### Worked context for the choices that benefit from it

**E2-V1 / E2-V2 — audience and bar.** The audience decides the GA showcase set
and the docs voice. Recommendation A keeps the beginner-first identity while
using "a small team ships a real internal service" (B) as the *proof*, not the
target market. Concretely, GA is "done" when this transcript is true on a
teammate's laptop with zero Jet experience:

```
$ jet run report.jet            # single file, no manifest — still works
$ jet new service && cd service # opt-in to a project only when you want one
$ jet test && jet dev           # instant feedback, real diagnostics
```

**E2-V4 — single-file sacredness.** This gates how much package ceremony new
users meet. Option A means the first program is always one file and `jet run`
never asks for a manifest, workspace, or config — packages are something you
*grow into*, surfaced only by `jet new`/`jet add`. This is load-bearing for the
beginner identity and is the recommended hard line.

**E2-V5 — concurrency lock.** Option A commits the whole epoch to tasks +
channels (already ratified S53) and forbids async creep. The honest positioning
to write into docs: *"Jet services scale like Go circa 2012 — thread-per-task is
right for the broad internal-service case; 100k-connection async is not us
yet."* Choosing A here lets E2-M10 size its performance claims truthfully.

**E2-V7 — services ambition.** Option B (small public APIs with TLS via a vetted
Rust library through the FFI tier) is the recommended ceiling. It rules out a
large web framework before the lower-level story is proven, but lets the GA
service showcase terminate TLS and call a real API.

## Group E2D — External release & versioning policy *(needed by E2-M2)*

| ID | Question | Options | Rec |
|---|---|---|---|
| E2-D1 | External release policy | A · B · C below | **A** |
| E2-D2 | What event flips on encoded Epoch SemVer, if ever? | A: never · B: at E2-M17 GA only · C: a separate launch after GA | **C** |

**E2-D1 options, with what `jet --version` prints under each:**

- **A — Normal SemVer until launch (Rec).** Compiler stays `0.x` → `1.x`; docs
  use "Epoch 2" for storytelling only. Beginner-friendly, SemVer tooling works.

  ```
  $ jet --version
  jet 0.9.0  (language epoch 2, edition 2026)
  ```

- **B — Adopt encoded Epoch SemVer now** (Anthony Fu's `EPOCH*1000+MAJOR`):

  ```
  $ jet --version
  jet 2000.0.0   # "Epoch 2, major 0" — powerful signal, beginner-hostile
  ```

  Strong launch story, but `2000.0.0` on a teaching toolchain fights priority #2.

- **C — Calendar/edition versioning for the *language*, SemVer for the *binary*.**
  Viable but needs extra policy; not recommended over A for now.

---

# Part 2 — Per-milestone owner gates

Each detailed plan lists these IDs under "Owner decisions". Every gate has a
*default if deferred* so an agent is never hard-blocked, but the owner should
confirm the load-bearing ones. Examples are inline where the call is non-obvious.

## Group M2 — Release policy & editions *(E2-M2)*

| ID | Question | Options | Rec / default-if-deferred |
|---|---|---|---|
| D-REL1 | Versioning policy | = E2-D1 | A (normal SemVer) |
| D-REL2 | Epoch-SemVer flip | = E2-D2 | C (separate launch) |
| D-REL3 | Project compatibility marker | A: `edition` field · B: `epoch` field · C: both · D: toolchain constraint only | **A — `edition`** |
| D-REL4 | LTS window length | A: 1 yr · B: 2 yr · C: no LTS pre-GA | **C until GA** |
| D-REL5 | Who may run migrations | A: owner-approved `jet fix` + edition upgrade only · B: any quick-fix may migrate · C: no auto-migration | **A** |

Example for D-REL3 (what a manifest declares, and the diagnostic when a toolchain
is too old):

```toml
[package]
edition = "2026"
```
```
error[E2001]: this package needs a newer Jet
  --> jet.toml:2:11
   |
 2 | edition = "2027"
   |           ^^^^^^ your toolchain supports editions up to 2026
help: upgrade with `jet upgrade`, or set edition = "2026"
```

## Group M3 — Developer command UX *(E2-M3)*

| ID | Question | Options | Rec / default |
|---|---|---|---|
| D-DX1 | `--json` diagnostic schema stability | A: stable & versioned by M3 exit · B: unstable/experimental in M3 | **A** |
| D-DX2 | `jet doctor` scope | A: rustc+cache+PATH+LSP+registry health · B: also auto-fix · C: minimal report-only | **A** |
| D-DX3 | Zed dev extension in Epoch 2? | A: yes, dev-tier · B: VS Code/Cursor only until GA | **A** (=E2-V9) |
| D-DX4 | Shell completions + man pages | A: ship in M3 from one source · B: defer to GA | **A** |
| D-DX5 | External subcommands (`jet-foo` → `jet foo`) | A: PATH discovery, no plugin API · B: none · C: full plugin API | **A** |
| D-DX6 | OSC 8 terminal hyperlinks on file:line / codes | A: when terminal supports it · B: never | **A** |

D-DX5 example (zero-cost extensibility, keeps I8 intact):

```
$ which jet-bench-compare        # any executable on PATH named jet-<x>
/usr/local/bin/jet-bench-compare
$ jet bench-compare old.json new.json   # dispatched to it, like cargo
```

## Group M4 — `jet dev` *(E2-M4)*

| ID | Question | Options | Rec / default |
|---|---|---|---|
| D-DEV1 | Interpreter coverage boundary | A: common programs; native-only set explained · B: attempt everything · C: expressions only | **A** |
| D-DEV2 | JIT in Epoch 2 | A: design-only note, no impl · B: implement (Cranelift, owner approval) · C: no JIT mention | **A** |
| D-DEV3 | Save-to-diagnostic latency budget | A: <200ms target w/ test · B: <500ms · C: no budget | **A** |

## Group M5 — Tier-2 references *(E2-M5)*

| ID | Question | Options | Rec / default |
|---|---|---|---|
| D-REF1 | Teaching order | A: after beginner ownership chapter · B: alongside ownership · C: appendix only | **A** |
| D-REF2 | Ship arenas this milestone | A: only if the parser example needs them · B: always · C: never in Epoch 2 | **A** |
| D-REF3 | Inlay-hint defaults beyond clone | A: borrowed-return + cleanup scopes on · B: clone only · C: all off by default | **A** |

## Group M6 — Library authoring *(E2-M6)*

| ID | Question | Options | Rec / default |
|---|---|---|---|
| D-LIB1 | S61 (labels/defaults) + S62 (delegation) timing | A: both in M6 · B: labels only, delegation later · C: delegation only | **A** |
| D-LIB2 | Generics step | A: associated types + default method bodies · B: also trait inheritance · C: also blanket impls | **A** |
| D-LIB3 | `?` error-conversion shape | = D-ERR2 (Group 14) | From-style `IntoError` trait |

## Group M7 — Streaming I/O & resources *(E2-M7)*

| ID | Question | Options | Rec / default |
|---|---|---|---|
| D-IO1 | Path handling | A: `std.path` helper module · B: first-class `Path` type · C: strings only | **A** |
| D-IO2 | Resource cleanup surface | A: RAII handle types (S63), drop on scope exit · B: explicit `close` only · C: `defer` keyword | **A** |
| D-IO3 | Keep whole-file `fs.read`/`fs.write` | A: keep as sugar over handles · B: deprecate · C: remove | **A** |

D-IO2 example (cleanup is automatic on every exit path; see also D-TXN/Group 18
and the `defer` discussion in D-SUGAR4):

```jet
fn copy(src: String, dst: String) -> Unit ? {
    val input = files.open(src)?;     // RAII handle
    val output = files.create(dst)?;  // both close on scope exit, even on `?`
    input.stream_to(output)?;
    ok(unit)
}
```

## Group M8 — Packages & supply chain *(E2-M8)*

| ID | Question | Options | Rec / default |
|---|---|---|---|
| D-PKGS1 | Registry hosting model | A: append-only git registry · B: hosted service · C: both | **A** |
| D-PKGS2 | `jet.*` namespace policy | A: owner-held reserved namespace · B: open · C: reserved + petition | **A** |
| D-PKGS3 | Signing | A: signed metadata optional v1, design signed cache · B: required · C: none | **A** |
| D-PKGS4 | Yank/immutability rules | A: immutable releases, yank hides from new solves · B: hard delete allowed · C: no yank | **A** |

## Group M9 — First-party library ring *(E2-M9)*

| ID | Question | Options | Rec / default |
|---|---|---|---|
| D-LR1 | First wave order | A: csv/toml/log/time first, then regex/archive/db · B: regex first · C: db first | **A** |
| D-LR2 | sqlite | A: via E2-M14 C FFI when ready · B: pure-Jet impl · C: defer db ring | **A** |
| D-LR3 | crypto surface | A: vetted hashes/HMAC/RNG only · B: also symmetric ciphers · C: also TLS primitives | **A** |

## Group M10 — Networking & services *(E2-M10)*

| ID | Question | Options | Rec / default |
|---|---|---|---|
| D-NET1 | TLS/HTTP dependency | A: rustls-class via FFI tier, never hand-rolled · B: openssl FFI · C: defer TLS | **A** |
| D-NET2 | Concurrency story for servers | A: blocking thread-per-task + channels (E2-V5) · B: small async exception · C: thread pool | **A** |
| D-NET3 | Service showcase backing store | A: sqlite-first · B: Postgres-first · C: file store | **A** |

## Group M11 — Testing, docs, bench *(E2-M11)*

| ID | Question | Options | Rec / default |
|---|---|---|---|
| D-TEST1 | Property testing | A: in, if a small design exists · B: required · C: defer | **A** |
| D-TEST2 | `todo` typed holes | = D-TOOL2 (Group 19) | defer unless small |
| D-TEST3 | Guided learning | A: `jet tour`/`jet learn` w/ real compiler feedback · B: docs-only · C: separate site | **B first, A if cheap** |
| D-TEST4 | Doctests run under `jet test` | = D-TOOL1 (Group 19) | **yes** |

## Group M12 — Debug & observability *(E2-M12)*

| ID | Question | Options | Rec / default |
|---|---|---|---|
| D-OBS1 | DAP timing | A: ship for VS Code/Cursor in M12 (before GA) · B: GA · C: post-GA | **A** |
| D-OBS2 | Panic local-value privacy | A: show safe locals only in dev mode · B: all locals · C: none | **A** |
| D-OBS3 | Metrics conventions | A: simple structured logs first; metrics OTel-aligned later · B: full OTel now · C: logs only | **A** |

## Group M13 — Expert low-level tier *(E2-M13)*

| ID | Question | Options | Rec / default |
|---|---|---|---|
| D-LL1 | I1 amendment wording | A: generated `unsafe` only inside user gates or vetted std internals · B: broader · C: no amendment (block M13) | **A** |
| D-LL2 | `unsafe` audit story | A: structured audit comment + lint · B: attribute · C: external tool | **A** |
| D-LL3 | `std.mem` API breadth | A: narrow (Ptr, alloc, layout, volatile) · B: wide · C: minimal Ptr only | **A** |

## Group M14 — C FFI *(E2-M14)*

| ID | Question | Options | Rec / default |
|---|---|---|---|
| D-CFFI1 | Jet-export to C in scope? | A: import-only first · B: also export Jet fns to C · C: defer all FFI to Epoch 3 | **A** |
| D-CFFI2 | Header/library discovery | A: pkg-config + classic flags from `[dependencies:c]` · B: bundled · C: manual paths only | **A** |
| D-CFFI3 | C example to ship | A: one small C lib (e.g. a hashing or compression lib) · B: sqlite · C: none | **A** |

(C-header auto-binding `jet bind` stays out — see docs/plans/post-epoch-2.)

## Group M15 — Cross-compile & freestanding *(E2-M15)*

| ID | Question | Options | Rec / default |
|---|---|---|---|
| D-CROSS1 | First non-host target | A: one CLI target (e.g. `aarch64-linux`) · B: a matrix · C: defer | **A** |
| D-CROSS2 | Freestanding panic strategy | A: abort default · B: custom handler hook · C: unwind | **A** |
| D-CROSS3 | Embedded smoke | A: documented local harness minimum · B: CI hardware-in-loop · C: doc-only, no smoke | **A** |

## Group M16 — Pure eval & layer 3 *(E2-M16)*

| ID | Question | Options | Rec / default |
|---|---|---|---|
| D-PURE1 | Recipe scope | A: pure eval + sandboxed package recipes · B: pure eval only · C: full JetOS | **A** |
| D-PURE2 | Sandbox guarantees | A: no ambient I/O or network during eval · B: allowlist I/O · C: trust author | **A** |
| D-PURE3 | Signed cache / rollback | A: design now, ship later; record generations · B: ship signed cache in M16 · C: none | **A** |

## Group M17 — Epoch 2 GA *(E2-M17)*

| ID | Question | Options | Rec / default |
|---|---|---|---|
| D-GA1 | Mandatory showcase set | A: 4 showcases + `jet dev` demo · B: all 6 (incl. C interop + freestanding) · C: 2 | **A, with B as stretch** |
| D-GA2 | Perf/size budgets | A: record per-showcase budgets, no hard fail · B: hard CI gates · C: none | **A** |
| D-GA3 | Beta period before GA tag | A: short public beta after audits · B: none · C: long beta | **A** |
| D-GA4 | Launch versioning | = E2-D2 | C (separate launch) |

---

# Part 3 — Feature ballots distilled from research

These were distilled from prior research exploration (Odin error handling,
Elixir pattern matching, the functional `pack.jet` debrief, and a CLI-tooling
survey) plus `docs/plans/owner-todo.md`. Those research files have been removed
now that their decisions live here; the Jetpack/JetOS design from the pack
debrief was migrated to `docs/plans/jetpack-jetos/pack-abi.md` (prior versions
are in git history). Nothing here is built until ratified (I7, I8). Each option
has a worked example.

## Group 14 — Error-handling ergonomics *(needed by E2-M6; some E2-M7)*

From the Odin case study. **Already ratified — do not re-ballot:** `T ? E`
(S34), the `T ?` → `T ? Error` shorthand and default `Error` type (S34), the
formatter rule `T?`→`T ?` with optional return `-> (T?)` (S34/spec.md), `?`
propagation (S7), `??`/`?.` (S71), `switch`/`panic`/`require`. These decisions
round out the *open* deltas only.

| ID | Question | Options | Rec |
|---|---|---|---|
| D-ERR1 | Grow the `Error` carrier (today it is backed by `String`) | A: message + optional code + optional source · B: message only · C: keep `String`-only | **A** |
| D-ERR2 | `?` cross-type conversion | A: opt-in `IntoError` trait; `String` + std errors convert by default · B: no conversion (manual wrap) · C: implicit any→any | **A** |
| D-ERR3 | `fn main() -> Unit ?` (today `main` may not be fallible) | A: allow, print `Error`, exit 1 · B: keep `main` non-fallible · C: allow only `Unit ? Error` | **A** |
| D-ERR4 | `or_continue`-style loop skip | A: defer · B: add `?continue` in loops · C: decline | **A (defer)** |

**D-ERR1 example** — a richer carrier lets `?` keep context as errors travel up,
while beginners still write `-> Config ?` and get the default type:

```jet
fn load_config(path: String) -> Config ? {          // == Config ? Error (S34)
    val text = fs.read(path)?;
    ok(parse_config(text)?)
}
// Error.message("…"), Error.code(2), Error.with_source(e) — open shape (D-ERR1)
```

**D-ERR2 example** — `?` converts lower-level errors into the function's error
type through an explicit, opt-in trait, so the happy path stays readable but
meaning is never erased silently:

```jet
impl FileError: IntoError {
    fn into_error(self) -> Error { Error.message("file error: {self}") }
}

fn load_profile(path: String) -> Profile ? {
    val text = fs.read(path)?;     // FileError -> Error (via trait)
    val data = parse_json(text)?;  // JsonError -> Error
    ok(Profile.from_json(data)?)
}
```

Option C (any error converts to any error automatically) is rejected as too
vague — it makes `?` convenient but erases which failure happened.

## Group 15 — Pattern-matching ergonomics *(post-v1; candidate E2-M5/M6 window)*

From the Elixir brief. **Already ratified — do not re-ballot:** `==` pattern
tests that destructure and bind in `switch`/`if` (S31), standalone destructuring
binds for struct/tuple/list with irrefutable struct + length-checked list (S74),
named tuples (S73), `??`/`?.` fallback and chaining (S71). These decisions cover
only the *open* deltas.

| ID | Question | Options | Rec |
|---|---|---|---|
| D-PAT1 | Nested patterns in `switch`/`if` arms | A: allow patterns inside payload slots (`ok(Rect(w,h))`) · B: bare names only | **A** |
| D-PAT2 | Guards | A: ratify `&&` binding-scope (bound names flow right + into the arm body) · B: add a `when` keyword · C: neither | **A** |
| D-PAT3 | Refutable-bind policy (e.g. `val value(n) = opt;` that can be `null`) | A: reject, teach `switch`/`if` · B: require `??` fallback · C: runtime panic (Elixir) | **B (A as the no-`??` error)** |
| D-PAT4 | List rest-spread patterns `[h, ...t]` (beyond S74 fixed-length) | A: defer (tail-copy perf mismatch on flat `List<T>`) · B: fixed-length stays the limit · C: full with slice design | **A** |
| D-PAT5 | Multi-clause function heads | A: decline for v1 (one obvious way; `switch` covers it) · B: add | **A** |
| D-PAT6 | Destructuring in *parameters* | A: defer (S74 bindings only for now) · B: also `fn f(Point { x, y }: Point)` | **A** |

**D-PAT1/D-PAT2 example** (the high-value, low-conflict bundle, building on S74):

```jet
switch response {
    response == ok(Response { status, body }) && status == 200
        -> { print("body: {body}"); };          // D-PAT2 guard via && scope rule
    response == ok(Response { status, body })    // D-PAT1 nested pattern
        -> { print("unexpected status {status}"); };
    response == err(e) -> { print("network error: {e}"); };
}
```

**D-PAT3 example** (refutable bind on an `Option` that might be `null` — note
S74 already made *struct* destructuring irrefutable; this is the enum/option
case):

```jet
val value(n) = maybe_port() ?? return;   // B: explicit failure path, reuses ??
val value(n) = maybe_port();             // A's teaching error if no `??`:
// error[E0xxx]: this binding can fail (the value might be null)
// fix: add `?? <fallback>`, or use `switch`/`if` to handle the empty case
```

## Group 16 — Field punning & functional config *(core; supports Jetpack)*

From the functional-pack debrief. These help all struct-heavy Jet, and unblock a
better-than-Nix `pack.jet`. **Jetpack-only** package-ref sugar (`default.[ripgrep,
fd]`, `github@…`, bare package names) stays in the jetpack-jetos track
(D-JPK*) — do not generalize it to core Jet. Listed here only so the boundary is
explicit.

| ID | Question | Options | Rec |
|---|---|---|---|
| D-FP1 | Struct field punning | A: `Source { name, upstream, via: "nix" }` when a local has the field's name · B: keep explicit `name: name` | **A** |
| D-FP2 | Expression-body functions | A: `fn f(x: T) -> U = expr;` · B: keep `{ return …; }` only · C: defer | **C (defer; lambdas already cover most)** |
| D-FP3 | Core `module name { … }` declaration | A: typed top-level decl lowering to a pure exported fragment · B: keep Jetpack directive scanning · C: defer | **A (Jetpack-scoped types)** |
| D-FP4 | Contextual empty-list inference | A: infer `[]` from expected/accumulator type in generic calls · B: require `[]: List<T>` | **A** |
| D-FP5 | Arbitrary expressions in `for … in <expr>` heads | A: allow field access/calls/indexing/ranges · B: keep restricted | **A** |
| D-FP6 | List spread `[...xs, y]` | A: defer (library `when`/`concat` first) · B: add general spread | **A (defer)** |
| D-FP7 | Jetpack package-ref sugar | tracked as D-JPK* in jetpack-jetos plan, not core | n/a here |

**D-FP1 example** (matches Nix `inherit`, with static field checking):

```jet
return Source { name, upstream, via: "nix" };   // not name: name, upstream: upstream
```

**D-FP3 example** (one declaration keyword; `Shell`/`Profile`/`System` stay
ordinary types — LSP parses it with no Jetpack-only grammar):

```jet
module root {
    sources: { default: github@NixOS/nixpkgs/nixos-24.05 },
    shells: { dev: Shell { packages: [default.[ripgrep, fd, jq]] } },
}
```

## Group 17 — Readability sugar *(cross-milestone)*

From `owner-todo.md` and the CLI-tooling survey. Small, mostly-free wins and two
explicit declines.

| ID | Question | Options | Rec |
|---|---|---|---|
| D-SUGAR1 | Digit separators `1_000_000` | A: add (lexer-only, free readability) · B: skip | **A (E2-M3 era)** |
| D-SUGAR2 | Pipe operator `\|>` | A: defer (S69 newline dot-chains cover it) · B: add · C: decline | **A** |
| D-SUGAR3 | Transparent type alias `type X = …` | A: defer · B: add · C: decline | **A** |
| D-SUGAR4 | Newtype (distinct single-field) | A: defer (one-field struct covers it) · B: dedicated keyword | **A** |
| D-SUGAR5 | `defer`/`errdefer` cleanup keyword | A: none — RAII handles (S63/D-IO2) are the model · B: add `defer` · C: add both | **A** |

D-SUGAR1 example: `val budget = 1_000_000;` parses identically to `1000000`; the
formatter never inserts or removes separators.

D-SUGAR5 note: the owner-todo lists `defer/errdefer` as a recurring need. The
recommendation is to satisfy it with RAII handle types (D-IO2) rather than a new
keyword, keeping "one obvious way". Reopen as B only if RAII proves insufficient
in E2-M7.

## Group 18 — Transactional rollback (`transact`) *(owner-flagged)*

From `owner-todo.md` §0.1 (Verse-inspired). Owner has flagged interest. Strong
philosophical fit ("leave the world consistent on error" becomes a language
guarantee), real implementation cost. Needs its own decision before any code.

| ID | Question | Options | Rec |
|---|---|---|---|
| D-TXN1 | Adopt `transact` at all? | A: block form tied to `?` · B: `fn … transacts` modifier · C: decline | **A** |
| D-TXN2 | I/O inside a transaction | A: compile error (only local owned mutation may roll back) · B: allow, document non-undoable I/O · C: warn | **A** |
| D-TXN3 | Snapshot cost posture | A: snapshot only mutated bindings, opt-in, never default path · B: snapshot whole scope | **A** |

**D-TXN1/2 example** — if any `?` inside the block short-circuits, every
in-memory mutation is rolled back as if it never happened; doing I/O inside is a
compile error so the guarantee stays honest:

```jet
fn try_move(player: mut Player, target: Point) -> Bool ? MoveError {
    transact {
        player.spend_stamina(10)?;   // mutates player
        player.step(target)?;        // may fail → spend_stamina is undone
    }
    ok(true)
}
```

## Group 19 — Tooling surfaces needing a decision *(E2-M3/M11)*

From the CLI-tooling survey. These each need a small owner call on the surface.

| ID | Question | Options | Rec |
|---|---|---|---|
| D-TOOL1 | Doctests | A: examples in doc comments run under `jet test` (I5 for user code) · B: docs not tested | **A (E2-M11)** |
| D-TOOL2 | `todo` typed-hole expression | A: compiles, panics at runtime, reports expected type · B: defer · C: decline | **B (defer unless small)** |
| D-TOOL3 | `jet emit --rust` expert window | A: gated "show our generated Rust" (framed as *our* output, not rustc's, re I2) · B: never | **A (owner call)** |
| D-TOOL4 | Snapshot testing in `jet test` | A: one-key bless like internal UPDATE_EXPECT · B: defer | **A (E2-M11)** |
| D-TOOL5 | Build-time capability summary from std imports | A: defer (honesty feature, deno-style) · B: add | **A (defer)** |

**D-TOOL3 tension to resolve (I2).** I2 says *rustc* never speaks to users.
`jet emit --rust` shows *our* generated Rust, not rustc's diagnostics, so it is
compatible — but it exposes the hidden backend. Recommendation A ships it as an
explicit expert/curiosity flag with a banner ("this is generated code; it is not
the language you write"). Owner confirms the framing.

---

## Group 12 — E2-M18 REPL *(open — see docs/plans/epoch-2/m18-repl.md)*

Interactive `jet repl` is planned for Epoch 2 as **E2-M18**, after the E2-M4
interpreter ships. No code until every ID below is ratified in
docs/spec/syntax-decisions.md (or deferred with a recorded default in the
plan). Recommendations are in the plan file.

| ID | Question (one line) | Rec |
|---|---|---|
| D-REPL1 | Ship terminal REPL in Epoch 2? | **A** — E2-M18 after E2-M4 |
| D-REPL2 | Web playground in this milestone? | **A** — terminal only |
| D-REPL3 | Entry: `jet repl` only vs bare `jet` in TTY vs seed file | **A** — `jet repl` only |
| D-REPL4 | Backend: interpreter vs compile-each vs hybrid | **A** — interpreter |
| D-REPL5 | Input: stmts vs full decls vs expressions only | **A** — stmts + control flow |
| D-REPL6 | Reject FFI/tasks/low-level vs also package imports | **A** — reject native-only set |
| D-REPL7 | Session: accumulating module vs cells vs both | **C** — accumulating + optional `:cell` |
| D-REPL8 | Ownership across lines: real moves vs auto-clone vs borrow-only | **A** — real move semantics |
| D-REPL9 | Multi-line: brace-count prompt vs `;` submit vs single-line | **A** — brace-count + `...` |
| D-REPL10 | Project context: sandbox vs auto `jet.toml` vs always sandbox | **A** — sandbox + `--project` |
| D-REPL11 | Line editor: std-only vs crate vs crate+completion | **B** — line-editing crate |
| D-REPL12 | vs `jet eval --pure`: separate vs `--pure` mode vs no REPL | **A** — separate commands |
| D-REPL13 | vs `jet dev`: independent vs flag vs shared process | **A** — share library only |
| D-REPL14 | Native snippet: reject vs temp compile-run | **A** — reject with workaround |
| D-REPL15 | Meta-commands: minimal vs +load/type/help vs +doc/imports/emit | **B** — +`:load` `:type` `:help` |
| D-REPL16 | Results: implicit echo vs type+value vs print-only | **A** — implicit echo, `;` suppresses |
| D-REPL17 | Diagnostics: identical vs shorter vs session context | **A** — identical to batch |
| D-REPL18 | Crate if D-REPL11≠A: rustyline vs reedline vs other | **A** — `rustyline` (I6) |
| D-REPL19 | Playground arch (if D-REPL2≠A): external vs in-binary vs defer | **C** — defer |
| D-REPL20 | Tests: transcripts vs +PTY vs manual only | **A** — transcript fixtures |
| D-REPL21 | Timing: separate M18 vs thin REPL in M4 vs Epoch 3 | **A** — separate E2-M18 |

Open follow-ups (not ballot IDs yet): interpreter fuel/timeout per input,
startup banner, color policy, implicit `import std` — see m18-repl.md § Open
questions.

---

## Tally sheet (open only)

| Group | IDs | Needed by | Status |
| --- | --- | --- | --- |
| E2V — strategic vision | E2-V1…V12 | before milestone scoping | ☐ |
| E2D — release/versioning | E2-D1, E2-D2 | E2-M2 | ☐ |
| M2 — release policy | D-REL1…5 | E2-M2 | ☐ |
| M3 — developer UX | D-DX1…6 | E2-M3 | ☐ |
| M4 — jet dev | D-DEV1…3 | E2-M4 | ☐ |
| M5 — references | D-REF1…3 | E2-M5 | ☐ |
| M6 — library authoring | D-LIB1…3 | E2-M6 | ☐ |
| M7 — streaming I/O | D-IO1…3 | E2-M7 | ☐ |
| M8 — packages | D-PKGS1…4 | E2-M8 | ☐ |
| M9 — library ring | D-LR1…3 | E2-M9 | ☐ |
| M10 — networking | D-NET1…3 | E2-M10 | ☐ |
| M11 — testing/docs/bench | D-TEST1…4 | E2-M11 | ☐ |
| M12 — debug/observe | D-OBS1…3 | E2-M12 | ☐ |
| M13 — low-level tier | D-LL1…3 | E2-M13 | ☐ |
| M14 — C FFI | D-CFFI1…3 | E2-M14 | ☐ |
| M15 — cross/freestanding | D-CROSS1…3 | E2-M15 | ☐ |
| M16 — pure eval/layer 3 | D-PURE1…3 | E2-M16 | ☐ |
| M17 — GA | D-GA1…4 | E2-M17 | ☐ |
| 14 — error ergonomics | D-ERR1…4 | E2-M6 | ☐ |
| 15 — pattern matching | D-PAT1…6 | E2-M5/M6 | ☐ |
| 16 — punning/config | D-FP1…6 | E2-M6 | ☐ |
| 17 — readability sugar | D-SUGAR1…5 | E2-M3+ | ☐ |
| 18 — transact | D-TXN1…3 | E2-M7 window | ☐ |
| 19 — tooling surfaces | D-TOOL1…5 | E2-M3/M11 | ☐ |
| 12 — E2-M18 REPL | D-REPL1…21 | E2-M18 | ☐ |
| — (deferred) | S56 | post-1.0 | ☐ |

---

## Already ratified (recorded elsewhere — do not re-list here)

Groups 1–11 and 13 are decided. Their content lives in the canonical sources,
not in this queue:

- **Groups 1–8** (S26–S64) — docs/spec/syntax-decisions.md.
- **Group 9** — D-PM1…8 — docs/plans/epoch-1/m12-packages.md.
- **Group 10** — D-LSP1…13 — docs/plans/epoch-1/m13-lsp.md.
- **Group 11** — D-JPK1…17 — docs/spec/syntax-decisions.md (plan:
  docs/plans/jetpack-jetos/README.md).
- **Group 13** — D-SG1…9 (syntax gallery) — docs/spec/syntax-decisions.md
  (S24/S22/S35/S42 amendments + S68–S74). Decided 2026-06-15.

Note: several research items are *already* ratified and must not be re-balloted
here — concurrency model (S53), `pure fn` (S60), labels/defaults (S61),
delegation (S62), RAII cleanup (S63), C FFI gate (S59). The ballots above decide
only their *timing and surface details* inside Epoch 2.
