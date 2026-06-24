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

> **Surfaced by the 2026-06-24 plan sweep (c121, c122).** Two implementation plans hit a
> real user-facing choice. Both are vetted against the codebase; the rest of each card's plan
> is unblocked.

### D-BENCH1 — how do you write a benchmark? (board card c121, rec A)

**Gist:** Pick the surface a developer uses to declare a benchmark.

**Story.** Walter ships a 2D game in Jet and his frame time is creeping up. `jet bench` already
times his *whole program* (warmups + trials, mean±stddev — `run_bench` in `CmdDevTools.rs`), but
he wants to measure his particle-update loop **in isolation**, watch that one region across
changes, and have `jet` report per-region ops/sec — the same way `#Test` names a single unit.

**In the wild:**

```jet
struct Particle { x: Float, y: Float, vx: Float, vy: Float }

fn step(p: ~Particle, dt: Float) {
    p.x = p.x + p.vx * dt
    p.y = p.y + p.vy * dt
}

// `jet bench` can time the whole binary, but Walter has no way to benchmark just this region in source.
```

**Other languages:**

```rust
#[bench] fn bench_step(b: &mut Bencher) { b.iter(|| step(&mut p, dt)); }   // Rust: attribute + Bencher
```
```go
func BenchmarkStep(b *testing.B) { for i := 0; i < b.N; i++ { step(p, dt) } }  // Go: naming convention + b.N
```
Rust uses an attribute, Go a naming convention. Jet already has a first-class `#Test "name" { }`
block (PascalCase attribute family `#Test`/`#Pure`/`#Todo`), so the question is whether benchmarks
join that family or take a different shape.

**Tradeoffs:** *(subagent-reviewed against Source/Parser/Items.rs `#Test` parsing + Syntax.rs)*

| Option | Surface | Sits beside `#Test`? | Discoverable | One-path (S14) |
|---|---|---|---|---|
| A `#Bench` block | `#Bench "name" { … }` | yes (sibling) | high | yes |
| B timing API | `core.time` / `bench("name", fn)` | no | medium | competes with `#Test` shape |
| C `#Test(bench:true)` | parameterised test attr | partial | low | overloads `#Test` semantics |

- **Option A — `#Bench "name" { … }` block (recommended).** A first-class benchmark block,
  the exact sibling of `#Test "name" { … }`; `jet bench` discovers and runs them, reporting
  ops/sec + ns/iter. PascalCase attribute, consistent with the `#Test`/`#Pure` family.
  ```jet
  #Bench "step 100k particles" {
      ps := make_particles(100_000)
      for p in ps { step(~p, 0.016) }
  }
  // $ jet bench
  // step 100k particles    1.84 ms/iter   543 iters/s
  ```
- **Option B — no new syntax; a timing API.** Benchmarks are ordinary functions calling a
  `core.time` stopwatch or a `bench(name, fn)` helper; no attribute.
  ```jet
  use core.time
  fn main() {
      t := time.stopwatch()
      for p in ps { step(~p, 0.016) }
      print("step: {t.elapsed_ms()} ms")
  }
  ```
  Con: every author hand-rolls the loop/reporting; no `jet bench` discovery; competes with the
  `#Test` shape beginners already know.
- **Option C — `#Test(bench: true) fn`.** A parameter on the test attribute marks a test as a
  benchmark.
  ```jet
  #Test(bench: true) "step" { for p in ps { step(~p, 0.016) } }
  ```
  Con: overloads `#Test` with two semantics (assert vs measure); the parameterised-attribute
  form isn't used elsewhere in Jet (a new sub-grammar for one flag).

**Recommendation:** **A** — `#Bench` is the one-path sibling of `#Test`, instantly discoverable,
and needs no new attribute grammar. **Owner Q — runner verb:** `jet bench` (rec, parallels
`jet test`) or fold into `jet test --bench`?

### D-PKGSIGN1 — what proves a published package is authentic? (board card c122, rec A)

**Gist:** Choose how a consumer verifies a package really came from its author (not just that
its bytes are intact).

**Story.** Doris publishes a crypto library; Hank pins it in his `pkg.jet`. A checksum proves
Hank got the bytes Doris uploaded — but not that *Doris* uploaded them (a compromised registry
could swap both bytes and checksum). Hank wants authenticity, offline-verifiable.

**In the wild:**

```jet
// pkg.jet — Hank depends on Doris's package
[deps]
hashing = "doris/hashing@2.1.0"   // today: resolved + checksum-pinned in the lockfile.
                                   // Authenticity (who signed it) is not enforced.
```

**Other languages:** npm/PyPI lean on registry TLS + checksums (integrity); crates.io adds
checksums; newer ecosystems (npm provenance, Go checksum DB, Sigstore/cosign) add
authenticity. Jet is std-only (I6) and offline-first by design — the constraint that shapes
this choice.

**Tradeoffs:** *(subagent-reviewed against Source/Lock.rs, Store.rs, Publish/ — `require_signed`
exists on RegistryConfig but is unenforced; no signing today)*

| Option | Proves | Offline-verifiable | External infra | Self-contained (I6) |
|---|---|---|---|---|
| A Ed25519 sign | authenticity + integrity | yes | none | yes — native Ed25519/SHA-512 in the ring layer |
| B checksum only | integrity only | yes | none | yes (already have SHA-256) |
| C Sigstore keyless | authenticity | no (needs Rekor/OIDC) | heavy | no |

- **Option A — Ed25519 author key pairs (recommended).** Author signs the package manifest +
  content hash with a private key; the public key is published/pinned; consumers verify
  offline. **Capability note:** `jet.crypto` today ships only SHA-256 — Ed25519 and SHA-512 do
  not exist yet, so under I6 this means a native Ed25519/SHA-512 implementation in the ring
  layer (or a `signify`/`ssh-keygen` subprocess), not a reuse of existing crypto. The existing
  SHA-256 checksum (B) remains the integrity baseline underneath.
  ```shell
  $ jet keygen                      # writes ~/.jet/keys/ed25519
  $ jet publish                     # signs manifest+hash; uploads signature
  # consumer:
  $ jet fetch                       # verifies signature against the pinned public key; refuses on mismatch
  ```
- **Option B — checksum-only (status quo+).** Keep hash pinning in the lockfile; no signing.
  ```shell
  $ jet fetch    # verifies sha256 matches the lockfile — integrity, not authorship
  ```
  Con: a compromised/malicious registry can substitute content + checksum; no authenticity.
- **Option C — Sigstore keyless.** Sign via OIDC identity; signatures logged to a transparency
  log (Rekor); no long-lived keys.
  ```shell
  $ jet publish    # OIDC browser flow; signature -> Rekor transparency log
  ```
  Con: requires network at publish + verify, an external transparency-log service, and a heavy
  dependency surface — at odds with Jet's std-only/offline-first stance.

**Recommendation:** **A** — Ed25519 gives offline-verifiable authenticity with zero external
infrastructure and keeps the existing SHA-256 checksum as the integrity layer beneath. It is
the only option that satisfies both "prove authorship" and "works offline / self-contained"
(the Ed25519/SHA-512 primitives are added natively to the ring layer, consistent with I6).
**Owner Q — key distribution:** publish author public keys in the registry index (TOFU on first
pin), or require an out-of-band key fingerprint in `pkg.jet`? Rec: TOFU + pinned fingerprint in
the lockfile.

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

