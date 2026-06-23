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

### D-MUTSELF1 — self-mutation in `mut self` methods

**Gist:** Let a `mut self` method change its own object — `self.count = self.count + 1` and whole-`self` `self = New{…}`.

**Story.** Marjorie Hale is porting her tabletop combat tracker to Jet. Her `Tracker` has a `mut self` method `take_hit(dmg)` that should knock `self.hp` down and bump `self.hits_taken`. She writes `self.hp = self.hp - dmg` and gets `E0003: this value can't be assigned to`. She tries rebuilding the whole receiver, `self = Tracker{…}`; that compiles through every check, then the build dies with a rustc internal-compiler-error banner (an I2 violation). No spelling works. She files it as "mutating methods don't mutate."

**In the wild:**
```jet
struct Tracker {
    hp: Int
    hits_taken: Int

    fn take_hit(mut self, dmg: Int) {
        self.hp = self.hp - dmg        // E0003: this value can't be assigned to
        self.hits_taken += 1
    }
}

fn main() {
    t := Tracker{ hp: 30, hits_taken: 0 }
    t.take_hit(7)
    print(t.hp)        // want 23
}
```
Today neither line is expressible: `self.field = v` is a hard parse error (E0003), and `self = Tracker{…}` parses + type-checks but ICEs in codegen (a known AST-path I2 hole — the `mut self` slot isn't dereferenced on the LHS).

**Other languages:**
```rust
// Rust — &mut self; field assignment is the normal form
fn take_hit(&mut self, dmg: i64) { self.hp -= dmg; self.hits_taken += 1; }
```
```swift
// Swift — method marked `mutating`; then self.x = v is allowed
mutating func takeHit(_ dmg: Int) { hp -= dmg; hitsTaken += 1 }
```
```python
# Python / TS — receiver always mutable, no marker
def take_hit(self, dmg): self.hp -= dmg; self.hits_taken += 1
```
Every mainstream language lets a method assign its own fields; Rust/Swift gate it behind a marker. Jet's marker already exists (`mut self`, S27); only the assignment is missing. S17 already says a reassignment LHS "must be a `var` or a `mut` parameter" — and `mut self` *is* a `mut` parameter — so the binding model already admits this; only codegen lags.

**Known bug to fix either way (the whole-`self` baseline).** `self = New{…}` is NOT a working baseline today — it ICEs (the `mut self` slot is built with `deref: false`, so it emits `self = …` on `&mut Self` → rustc rejects). The owner has sanctioned it; the fix is to give the `Mutate` self-slot `deref: true` so `place_of` renders `*self = …`. No new syntax. In scope regardless of the field-mutation choice below.

**Tradeoffs:**

| Option | Memory-safe | Beginner-clear | Ceremony | New syntax | Impl surface |
|---|---|---|---|---|---|
| A — `self.field = v` direct *(recommended)* | yes (I1: `mut` discipline + rustc verify) | high — mirrors every language | none | none | parser `LValue::Field` + sema mut-check + codegen + TIR |
| E — return-new-self builder *(already works)* | yes | medium | medium — caller rebinds | none | none (ships today) |
| Reject — status quo | yes | n/a | n/a | none | none |
| B — mutate via a `mut`-bound local | yes | low — hidden write-back | high | none | sema rebind |
| C — whole-`self` functional update only | yes | medium | high (restate every field) | **yes — struct spread** | parser+codegen |
| D — typed `with` update | yes | medium | medium | **yes — `with`** | large |

- **Option A — `self.field = v` direct.** *(recommended)* Inside a `mut self` body, `self.field = v` is legal and lowers to `(*self).field = v` on `&mut Self`. Lifts E0003 for `self`-rooted field targets, but only inside a `mut self` method: the parser gains a field-lvalue and sema checks the receiver convention. Compound `+=` (S17) comes free on the same lvalue path.
```jet
fn take_hit(mut self, dmg: Int) {
    self.hp = self.hp - dmg     // (*self).hp = …
    self.hits_taken += 1        // S17 compound, same path
}
```
The same line in a default (shared-read) `self` method, or a call on a non-`mut` binding, errors in the ownership voice (new code E0205):
```shell
Error [E0205]: `take_hit` changes the tracker, so `self` must be `mut self`
  --> game.jet:6:9
   |
 6 |         self.hp = self.hp - dmg
   |         ^^^^^^^ this assigns a field of `self`
 Why: a method may change its receiver only when its receiver is marked `mut self`
 Fix: write the receiver as `mut self` — `fn take_hit(mut self, dmg: Int)`
```
(I4: this new diagnostic ships with a `tests/ui/` snapshot.)

- **Option E — return-new-self builder.** *(already works today)* A `self` method returns a fresh value; the caller rebinds. No mutation, no change needed — the current status quo for "changing your object."
```jet
fn take_hit(self, dmg: Int) -> Tracker {
    return Tracker{ hp: self.hp - dmg, hits_taken: self.hits_taken + 1 }
}
// caller: t @= t.take_hit(7)
```
Works now, but the caller must rebind and the method restates every field.

- **Reject — status quo.** Keep self-mutation an error; users reach for Option E or thread a `mut` local. The I8 fork: is in-place mutation worth a feature, or is return-new-self enough? (The recommendation says A is worth it — mutating methods are a core, every-language idiom and Blueprint-friendliness is the north star.)

- **Option B — mutate via a `mut`-bound local.** `self.field = v` stays banned; copy `self` into a `:=` local, mutate, write back. More moving parts; the write-back point is invisible. Reads worse than A for zero safety gain.
```jet
fn take_hit(mut self, dmg: Int) {
    s := self
    s.hp = s.hp - dmg
    self = s            // explicit write-back
}
```

- **Option C — whole-`self` functional update only.** Field-assign stays banned; every change is a replacement built from old `self`. With no spread syntax this restates every field (forget one → silently reset). A terse form needs **new struct-spread syntax** (`{ …self, hp: … }`, currently unratified).
```jet
fn take_hit(mut self, dmg: Int) {
    self = Tracker{ hp: self.hp - dmg, hits_taken: self.hits_taken + 1 }
}
```

- **Option D — typed `with` update.** A dedicated copy-and-override expression. Cleaner than C but introduces a **new `with` construct** end-to-end to do what A does with no new surface.
```jet
fn take_hit(mut self, dmg: Int) {
    self = self with { hp: self.hp - dmg }
}
```

**Recommendation:** Option A, plus the whole-`self` `*self =` fix. It is the form every language uses, needs no new syntax (S17 already admits a `mut` LHS), stays fully memory-safe (the `mut` borrow discipline + rustc verify are unchanged — I1 holds), and is the most Blueprint-friendly: a mutating method just assigns its fields. Option E already covers the functional path for free; C and D buy nothing A doesn't, at the cost of new owner-facing syntax. Ratifying A (and the `*self =` fix) unblocks deleting the legacy AST codegen path (c109 Phase N).

**Owner Q1 — read-only diagnostic.** When `self.field = v` appears in a non-`mut` method, should E0205 point at the assignment ("add `mut self`") or at the signature? Proposed: at the assignment (the fix is one token on the receiver).

**Owner Q2 — confirm scope.** A is in-place mutation; B's copy-in/copy-out is NOT proposed as a parallel allowed form. Confirm you don't also want B.

---

> **Drained 2026-06-22.** The owner's 2026-06-22 batch ratified every open full card —
> D-UNSAFE2, D-FIXARR1, D-CAP2/3, D-EFF2/3, D-MIGRATE2A/B/C/D/E/F, D-JSONOUT1, D-ARGS1,
> D-MATHLIB1, D-SIMD1, D-REACT1, D-FANOUT2, D-STRPARSE1, D-CTCORE1, D-JIT1, D-HOTSWAP1,
> D-DEVMODE1, D-SOA2A/B/C/D, D-TEST1, D-TEST4, D-BIND2, D-NUMOPS1, D-SERDE1, D-ITER1 (plus
> the earlier batch D-EFF1/D-QUAL1/D-TXN1/D-MIGRATE1/D-SOA1 and D-DBG2). All are recorded
> in `syntax-decisions.md` and their cards stripped from this file. The effect-system
> surface is now fully decided (D-EFF1+D-QUAL1+D-EFF2+D-EFF3). As of that batch no full
> cards remained open; **D-MUTSELF1 (self-mutation in `mut self` methods, above) was opened
> 2026-06-23** and is the one full card now awaiting decision. What's left below is
> informational only: the **deferred-ballots list**
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

