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

**D-FILES-APPEND1 — `core.fs`/`core.files` merge: the `append` name collides (board card cv5syntaxdecrees, D-FILES-WRITE1).**

**Gist:** Two different stdlib modules both want the method name `append`, with different meanings — merging them under `core.files` needs one renamed.

**Story.** Walter Higgins is porting a log-rotation script to Jet. He already has whole-file code using `core.fs.append(path, "line\n")` to tack one line onto a file, and now wants to switch a long-running job to `core.files.open(path)` streaming handles for a hot loop — he expects both to live under the same module name once D-FILES-WRITE1 lands (`core.files` is decreed as *the* file module).

**In the wild:**
```jet
use core.files as fs

// Whole-file convenience — one call, opens+writes+closes:
fs.append("audit.log", "user logged in\n")

// Streaming handle — many writes, one open/close:
h :: fs.open("audit.log")
h.append("user logged in\n")   // same method name, different receiver/arity
h.close()
```
Both spellings already exist in the compiler today, in two separate modules
(`core.fs.append(path, text)` — 2-arg whole-file; `core.files`'s handle type
has a 1-arg `.append(text)` method after `.open`/`.create`). D-FILES-WRITE1
(ratified) says `core.files` becomes the one file module and folds `core.fs`
into it — but a single Rust match arm can't dispatch `(module="files",
method="append")` two different ways by argument count without a form of
overloading the language doesn't otherwise use (I8 risk).

**Other languages:**
```rust
// Rust: two different types own "append" — no collision, because the whole-
// file convenience doesn't exist as a free function; it's OpenOptions::append.
std::fs::OpenOptions::new().append(true).open(path)?;
```
```typescript
// Node fs/promises: whole-file convenience is a different verb (appendFile),
// handle-based writes use write() — no collision because names differ.
await fs.appendFile(path, text);
await handle.write(text);
```
```python
# Python: no whole-file one-shot append built in; open(path, "a") then write().
with open(path, "a") as f:
    f.write(text)
```
Prior art leans toward *different verbs* rather than same-name overload by
receiver — none of Rust/Node/Python overload one method name across a
one-shot free function and a handle method the way the current two Jet
modules do.

**Tradeoffs:**

| Option | One-way-to-mean-it (I8) | Migration cost | Reads naturally |
|---|---|---|---|
| A — rename whole-file to `append_all` (recommended) | Clean: `append` is always the handle method, `append_all` is always whole-file | Low — only 1 stdlib fn + any examples/tests using `core.fs.append(path, text)` | `fs.append_all(path, text)` vs `h.append(text)` — clear at a glance |
| B — rename handle method to `write` (drop `append` from handles) | Clean, but `.write` already means "overwrite from start" elsewhere in `core.files` per D-FILES-WRITE1's own text ("handle method is `.write`") — would collide with THAT | Low | Ambiguous — does `.write` append or overwrite? |
| C — keep both named `append`, dispatch by arity | Violates I8 (silent overloading, no precedent elsewhere in Jet) | Zero | Confusing — same name, different behavior class |

**Worked example of every option:**

- **Option A — rename whole-file convenience to `append_all` (recommended).**
```jet
use core.files as fs

fs.append_all("audit.log", "user logged in\n")   // one-shot, opens+writes+closes

h :: fs.open("audit.log")
h.append("more\n")                                // streaming handle, unchanged
h.close()
```
- **Option B — rename handle method, keep whole-file `append`.**
```jet
use core.files as fs

fs.append("audit.log", "user logged in\n")        // whole-file, unchanged name

h :: fs.open("audit.log")
h.write("more\n")                                  // but D-FILES-WRITE1 already
                                                    // says .write is the OPEN/
                                                    // OVERWRITE handle method —
                                                    // this collides with that
h.close()
```
- **Option C — keep one name, dispatch by argument count (rejected, shown for completeness).**
```jet
use core.files as fs

fs.append("audit.log", "user logged in\n")   // 2-arg: whole file
h :: fs.open("audit.log")
h.append("more\n")                            // 1-arg: handle — same name, silently different job
```

**Recommendation:** A. Rename the whole-file one-shot function to `append_all`;
keep `append` as the streaming-handle method (unchanged). Zero ambiguity, one
name change, smallest migration, no collision with D-FILES-WRITE1's own
`.write` decision for handle overwrite. Blocks: card cv5syntaxdecrees /
D-FILES-WRITE1 core.fs→core.files merge (found 2026-07-04, decrees 1-3 of
that card ship independently, this is decree 5's only blocker).

---

The 2026-07-03 quick round (STYLEUNIT1=A, UIDEVSHELL1=A,
OSTARGET2=B, EXPANDCLI1=A, MIGRATE4=A) is ratified in `syntax-decisions.md`;
D-FLAGSHIP1–4 deferred to end of Epoch 4 by owner directive.
(D-E4EXIT1 / D-BUILDFLAGS1 on card #95 stay ratified-pending-build.)

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
