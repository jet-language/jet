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

## Open decisions — 2026-07-03 quick round

Five ballots, written short on purpose. Answer with a letter each (e.g.
"STYLEUNIT1=A"); full facet cards live in Tower Focus Mode if you want the
long version. (D-E4EXIT1 / D-BUILDFLAGS1 on card #95 stay ratified-pending-build.)

### 1. D-STYLEUNIT1 — how do you write "320 pixels" in a UI style? (card #134)

```jet
style.{ width: 320px }                        // A — unit literal (recommended)
style.{ width: .Px.{value: 320.0} }           // B — enum variant
style.{ width: Length.{value: 320.0, unit: .Px} }  // C — struct pair (what Phase 3 shipped)
style.{ width: 320.0 }                        // D — bare Float, pixels by convention
style.{ width: "320px" }                      // E — CSS string, parsed at runtime
```

**A** reuses the one ratified unit mechanism (`#UnitFamily` + `320ms`-style
literals) — compile-checked, zero ceremony, no second unit system. **D** is
least typing but the unit lives in docs only; **E** turns typos into runtime
failures. **Recommendation: A.**

**Answer:** ___

### 2. D-UIDEVSHELL1 — native UI backends need GTK headers; how do they get in? (card #134)

Phase 8 (real Linux/macOS windows) can't link today: the dev shell has no
toolkit libs. Nothing an agent does can proceed until you pick the route.

- **A — nixpkgs devShell deps now (recommended).** Add `gtk4` + `pkg-config`
  to the flake (your existing native-deps stopgap rule). Linux backend ships;
  macOS/Windows wait for those hosts.
- **B — defer Phase 8 to Epoch 4.** Epoch 3 UI stays web-renderer only;
  native lands with the jetpack core provider.
- **C — headless native backend first.** Stub renderer, no toolkit linkage;
  exercises the seam, real windows later via A or B.

**Answer:** ___

### 3. D-OSTARGET2 — how does plain `main` get "the impl for this OS"? (card #134)

`#Target(Os.Linux)` now gates impls (shipped tonight), but ungated code has
no blessed way to construct whichever impl survived the build.

```jet
// A — fn-level gating, one survivor per build (recommended; Rust cfg / Go build-tags shape)
#Target(Os.Linux) fn backend() -> JetBackend { return LinuxBackend.{} }
#Target(Os.Macos) fn backend() -> JetBackend { return MacosBackend.{} }
fn main() { app.mount(backend()) }   // ungated call resolves to the survivor

// B — comptime switch (new surface, second way to say A — I8 pressure)
b :: match build.os { .Linux -> LinuxBackend.{}  .Macos -> MacosBackend.{} }

// C — status quo: every call chain from main must itself be OS-gated (main per OS)
```

**Recommendation: A.**

**Answer:** ___

### 4. D-EXPANDCLI1 — exact shape of the transparency command (card #183)

Three ratified decisions promised a "show me what the compiler proved" lens
(inline decisions, resolved ref owners). The spelling:

```shell
jet expand --facts inline main.jet    # A — ratified wording; bare `jet expand` shows ALL lenses (recommended)
jet expand --facts inline main.jet    # B — same flag, but bare `jet expand` prints a lens menu + exits nonzero
jet expand inline main.jet            # C — positional lens (amends 3 ratified texts)
jet facts inline main.jet             # D — new subcommand (amends 3 ratified texts)
jet semindex main.jet --facts inline  # E — fold into semindex
jet check main.jet --facts inline     # F — hang off jet check
```

**A** honors the ratified wording verbatim; bare-shows-everything is the
magic default (beginner needs no lens names). Deciding this unblocks the
deferred lenses from #166/#174/#134. **Recommendation: A.**

**Answer:** ___

### 5. D-MIGRATE4 — should decode actually apply `migration {}` blocks to old data? (card #184)

Found while shipping #105: migration blocks are compile-time checks only —
codegen erases them. Old stored JSON still fails to decode, and
`decode_traced` can never report `migrated: true`.

```jet
// today: v1 JSON -> DecodeError, always
user :: json.decode<UserRecord>(raw)?
```

- **A — generate the migration chain into decode (recommended).** Old data
  loads and upgrades silently; `decode_traced` reports from+steps. Zero cost
  for types without migrations. The semantics both ratified surfaces
  (migration verbs, decode_traced) were designed around.
- **B — separate `decode_migrating` verb.** Plain decode keeps failing on old
  shapes; three decode verbs to learn (I8 pressure).
- **C — defer past Epoch 3.** Stored-data users hand-write upgraders;
  decode_traced stays always-false.

**Answer:** ___

---

## Recently ratified — context (no action)

_**2026-06-26 batch:** **D-COMPILERSEAMS2** (A — `jet-<seam>` technical naming: `jet-foundation`, `jet-lexer`, `jet-parser`, `jet-sema`, `jet-codegen`, `jet-comptime`, `jet-driver`) · **D-CTFIND2** (A — hand-rolled glob, owner-expanded to full spec including `{a,b}` brace expansion and `[abc]` character classes — "not something half assed") · **D-HTTPLIB1** (A — function-first mux: `fn(req) -> Response` + `mux.get`) · **D-HTTPLIB2** (B — split `jet.http.client` / `jet.http.server`) · **D-HTTPLIB3** (C — HTTP/1.1 + HTTP/2 + WebSocket in v1) · **D-HTTPLIB4** (B — rustls pure-Rust TLS, zero system dep). All recorded in `syntax-decisions.md`. c157 (find/glob) + c160 (compiler seams) + c164 (HTTP library) unblocked._

## Previously ratified — context (no action)

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
over the aviation menu) · **D-METADERIVE1** (A — `derive T.Trait` + source-fragment
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
