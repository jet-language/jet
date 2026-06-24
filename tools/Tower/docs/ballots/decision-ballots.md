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

_Two open cards (below): the effect-tag **vocabulary** (D-EFF4) and **lattice shape**
(D-EFF5), developed from the owner's D-EFF1 = B instruction ("crosscheck the subquestions,
create non-duplicate ballots"). The 2026-06-24 batch-4 drain (note below) cleared the serde
derive-path / dispatch / formatter / SIMD / embedded cards. The owner's **D-SOA1** follow-on
asks were already developed + ratified as D-SOA2A–D on 2026-06-22 (name → `#layout(columnar)`,
whole-struct-only v1, reserve `columnar [Particle]`, serialization-transparent), so no SOA
cards are open._

**Still deferred (not blocking; expand to a card when needed):**
- **D-SERDE-ACCESS — dynamic-tree accessor API.** How a user reads an untyped
  `Json`/agnostic `DataTree` by hand: pattern-match (shipped today) vs a fluent accessor
  (`tree.field("x").int()?`, `.text()`, `.bool()`, indexing). Only matters for the
  hand-impl / dynamic path (D-SERDE2), not the typed derive. Recommend: keep
  pattern-match as the floor; add minimal fluent accessors if hand-impl ergonomics demand it.

---

## An effect system, expressed as tags on functions — board card c62

### D-EFF4 — The named effect vocabulary: which effects exist, and is the set closed? (rec A)

**Gist:** Pin the exact list of built-in effect names and decide whether users may mint their own.

**Story.** Walter, who maintains a payments library, writes `fn charge(card: Card) #(Net, Db)` and expects the compiler to know `Net` and `Db` as effects — but he also wants to express "this touches our HSM hardware module," which is none of the ten built-ins. Whether he can write `#(Hsm)` and have it mean something, or must overload `Exec`, depends entirely on whether the effect set is closed.

**In the wild:**
```jet
// A driver author wants a domain effect the ten built-ins don't cover.
fn sign(payload: Bytes) #(Hsm) {        // is `Hsm` a known effect, or E0119?
    hsm.sign(payload)
}

// A reviewer reads a signature and must know the full vocabulary it can name.
fn audit_export(rows: [Row]) #(Fs, Net) {   // exactly which words are legal here?
    write_csv(rows)
    upload(rows)
}
```

**Other languages:**
```text
Koka    — effects are OPEN: `effect raise { ctl raise(msg): a }`; rows carry arbitrary labels
Frank   — open ability set, user-declared interfaces
Haskell — mtl/effectful/polysemy: fully open; an effect is a user-defined type/class
Swift   — CLOSED: exactly two ambient effects (`throws`, `async`); no third can be added
Rust    — no effect system; nearest analog (`const`/`unsafe`/`async`) is a fixed compiler set
```

**Tradeoffs:** (subagent-reviewed)

| Option | Beginner clarity | One-path | Reviewer can read sig | Covers domain effects | Safe-by-default |
|---|---|---|---|---|---|
| A — closed built-in set | high (finite, LSP lists all) | strong | always — vocabulary is fixed | only via the built-ins | strong (no unknown labels) |
| B — closed core + reserved-extensible later | high now | strong now | yes | not yet, door open | strong |
| C — open / user-declarable now | lower (unbounded vocabulary) | weaker (two ways to say a thing) | needs import resolution | yes | weaker (label means nothing to caller without def) |

- **Option A — closed built-in set (recommended).** Exactly the ten the implementation already carries (`Net, Fs, Io, Db, Time, Rand, Env, Exec, Log, Gpu`); `#(Hsm)` is E0119 "isn't a known effect."
  ```jet
  fn fetch(url: Url) #(Net) { http.get(url) }      // ok
  fn sign(p: Bytes) #(Hsm) { hsm.sign(p) }         // E0119: `Hsm` isn't a known effect
  fn sign(p: Bytes) #(Exec) { hsm.sign(p) }        // ok — fold the device into the closest built-in
  ```
- **Option B — closed core now, reserve user-extensible effects as a future spelling.** Ship A's ten, but pre-commit that a later `effect Hsm` declaration form is reserved (no syntax minted today).
  ```jet
  fn sign(p: Bytes) #(Exec) { hsm.sign(p) }   // today: same as A
  // RESERVED for a future ballot — not legal yet:
  // effect Hsm
  // fn sign(p: Bytes) #(Hsm) { hsm.sign(p) }
  ```
- **Option C — open / user-declarable effects now.** A user mints `effect Hsm` and the row machinery carries it like a Koka label.
  ```jet
  effect Hsm                                  // user declares a new effect
  fn sign(p: Bytes) #(Hsm) { hsm.sign(p) }    // legal; `Hsm` now a known label in scope
  ```

**Recommendation:** A — the ten built-ins already cover every Core-backed source of ambient power, a finite vocabulary is the only one a reviewer can read without import resolution, and a closed set keeps safe-by-default airtight; user-extensible effects are real but are their own ballot, not a thing to ship by accident in code.

**Owner Q — the exact ten.** The implementation carries `Net, Fs, Io, Db, Time, Rand, Env, Exec, Log, Gpu`. Right ten? Candidates: `Async`/`Spawn` (concurrency as an effect?), `Panic`/`Abort` (divergence, like Koka's `div`?). `Unsafe` is already a separate gate (D-LL1) — recommend NOT an effect. Recommend shipping the ten as-is; concurrency/divergence effects are a separate follow-on if wanted.

**Owner Q — extensibility door.** If you lean A but want the door open, that's Option B. Pick B only if domain effects (hardware, custom capabilities) matter before v1; otherwise A, and reopening later costs nothing.

---

### D-EFF5 — Effect lattice shape: is `Io` an umbrella over `Net`/`Fs`, or a flat sibling? (rec A)

**Gist:** Decide whether the ten effects are a flat set or a hierarchy where coarse effects subsume finer ones.

**Story.** Mabel writes a logging helper and tags it `#(Io)` because it does "some I/O." A caller declares `fn handler() #(Io)`. Later someone adds a network call inside Mabel's helper. Should that compile (because `Net` is a kind of `Io`)? Or fail (because `Io` and `Net` are unrelated siblings and the caller never authorized `Net`)? The answer changes whether `#(Io)` is a safe blanket or a precise claim.

**In the wild:**
```jet
fn log_line(s: String) #(Io) {     // Io = the print/input console effect
    print(s)
}

fn report(rows: [Row]) #(Io) {     // author thinks "Io covers it"
    log_line("starting")
    upload(rows)                    // adds Net — does #(Io) cover this, or E0740?
}
```
Today (`Source/Sema/Effects.rs`): `Io`, `Net`, `Fs` are flat siblings — `print` → `Io`, `http.get` → `Net`, `fs.read` → `Fs`. So `report` is **E0740 (Net outside #(Io))** under the current flat lattice. This card asks the owner to ratify that, or choose a hierarchy.

**Other languages:**
```text
Koka            — flat labels; `console`, `ndet`, `div` independent rows, no subsumption  (≈ Option A)
Swift           — flat (the two effects don't subsume each other)
Haskell effectful— flat; `IOE` is one effect, finer effects are independent — no "IO subsumes all"
E / Pony (ocaps)— often hierarchical (a coarse capability dominates finer ones)            (≈ Option B)
```

**Tradeoffs:** (subagent-reviewed)

| Option | Beginner mental model | Precision of a signature | Safe-by-default | Reviewer surprise |
|---|---|---|---|---|
| A — flat, all ten independent | "list every kind you do" — literal | high — `#(Io)` means only console | strong — `Net` never hides under `Io` | low — what you wrote is what's checked |
| B — `Io` umbrella over `Net`/`Fs`/console | "Io = any I/O" — fewer words | low — `#(Io)` silently admits `Net`/`Fs` | weaker — a net call hides under a blanket | high — `#(Io)` authorizes more than it reads |
| C — flat + explicit `#(AnyIo)` blanket alias | simple + an opt-in shortcut | high by default, blanket only when asked | strong (blanket is explicit) | low |

- **Option A — flat lattice, all ten independent (recommended).** No subsumption; `#(Io)` means console only. A `Net` call under `#(Io)` is E0740.
  ```jet
  fn report(rows: [Row]) #(Io, Net) {   // must name both — each effect is its own claim
      log_line("starting")              // Io
      upload(rows)                      // Net
  }
  ```
- **Option B — `Io` is an umbrella over `Net`/`Fs`/console.** Declaring `#(Io)` admits any finer I/O effect.
  ```jet
  fn report(rows: [Row]) #(Io) {        // Io blanket-covers Net + Fs + console
      log_line("starting")
      upload(rows)                      // Net — allowed, it's "a kind of Io"
  }
  ```
- **Option C — flat by default plus an explicit `#(AnyIo)` blanket alias.** Precision stays the default; a caller who wants "any I/O" opts in by name.
  ```jet
  fn glue() #(AnyIo) {                  // explicit blanket — expands to all I/O-family effects
      upload(rows)                      // Net, covered by the named blanket
      fs.write(path, bytes)             // Fs, covered
  }
  ```

**Recommendation:** A — a flat lattice is what the implementation already does, what Koka and the mainstream effect libraries do, and the only shape where a signature's effect list means exactly what it says; an umbrella `Io` quietly authorizes `Net`/`Fs` a reviewer can't see in the word `Io`. If "any I/O" ergonomics ever bite, C adds an opt-in blanket without weakening the default.

**Owner Q — the console effect's name.** Under flat (A), the `print`/`input` console effect is currently named `Io`. Since it no longer means "all I/O," consider renaming it `Console` or `Stdio` so `#(Io)` doesn't read like a blanket it isn't. Recommend `Io` → `Console`.

---

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

**D-JITDEP1 — Approve the Cranelift runtime dependency for the JIT tier-1 (D-JIT1 D+).**
*User story:* A dev runs `jet serve` on a large resident program; the tier-0 comptime
interpreter is correct but slow, and live-heap-preserving hot-swap (true state retention
across an edit) needs a real JIT. D-JIT1 was ratified as **D+** — Cranelift JIT "this
Epoch-3," not deferred — but the Cranelift crate is a runtime-side dependency that needs a
**separate owner dep-approval** (an I6 runtime exception, like the regex/`D-REGEX1` bootstrap).
*Decision (when promoted):* approve (or decline) adding `cranelift` as a runtime-side dep so a
`CraneliftBackend` can implement the already-shipped `JitBackend` seam as tier-1; the compiler
(`Source/`) still takes no crate (I6 holds — the dep lives runtime-side only). Rec direction:
**approve**, scoped + owner-signed like D-REGEX1, with the standing I6 obligation noted.
*Why surfaced now:* the prerequisite (the `JitBackend` seam) **shipped with c77** (commit
efd09d1), so this is the sole remaining gate on the JIT tier — promote to a full card when
you're ready to take on the Cranelift integration. Recorded so it isn't lost when c77's done
card is retired (the seam + interpreter tier-0 are complete and durable in
syntax-decisions.md D-JIT1 + spec).

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

