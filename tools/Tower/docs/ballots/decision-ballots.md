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

_Two cards open for the owner: **D-STATE-DECL** (typestate state vocabulary — loose tags vs enum) in the D-STATE1 scratch section below, and **D-JIT2** (where the Cranelift dependency physically lives)._

---

# Ballot scratch — D-STATE1 typestate surface spelling forks

D-STATE1 (=A) is ratified: *typestate via transitioning tags — a fn takes the old
state tag and returns the next; wrong-state call = compile error E0150; tags erase,
zero runtime cost.* The **mechanism** is pinned. What the one-line ratification does
NOT pin is the exact owner-facing **spelling** of three surface elements. Per the
syntax-decision protocol I built the clearly-implied core (E0150, erasing tags,
forward state dataflow) using the established marker idioms, and queue the spellings
below for owner confirmation. The implemented spellings are the defaults; nothing
about the mechanism changes if the owner picks an alternative — only the lexer/parser
spelling and a re-bless.

The state *value-fact prefix* (`#Pending res`) is NOT in question — it is the
ratified D-QUAL1/D-TAINT1 value-fact-rides-the-value idiom (`#Tainted expr`),
already shipped. Only the two fn-modifier markers and the arrow glyph below are forks.

---

### D-STATE-DECL — how is the state set declared? `state Reservation { … }` block (rec B — owner-directed 2026-06-25)

**Owner's direction (2026-06-25):** "Isn't one of the markers called `#State`? For D-STATE-DECL we could just do option B with `state Reservation { Pending, Confirmed, CheckedIn }`." Adopted as the recommendation. Two clarifications it settles:

1. **`#State` vs `state` is cohesion, not collision.** Yes — `#State(CheckedIn) fn` is the ratified require-state *marker* (D-STATE-REQ=A). A bare `state Reservation { … }` *declaration* shares the word on purpose, and it's the **same bare-keyword-declaration vs `#`-marker split Jet already uses everywhere**: `tag Foo {}` declares / `#Tainted` marks; `struct Foo {}` declares / `#Codable` marks. So the three read as one family — **`state` declares the set, `#State(X)` requires one, `#Transition(A -> B)` moves between them.** One word, three consistent forms.

2. **Why a dedicated block and not an enum.** (Background, since the earlier reframe weighed an `enum`/`comptime enum` vocabulary.) An `enum` is conceptually a *runtime value* type; using it purely as an erasing typestate vocabulary is a semantic stretch (you'd need a `comptime enum` qualifier to signal "this one has no runtime form"). A purpose-built `state TypeName { … }` says exactly what it is, ties the set to the type it governs by name, erases by definition, and pairs cleanly with `#State`/`#Transition`. It costs one small new keyword — worth it for the cohesion and for not overloading `enum`. (If a user *does* want a queryable/serializable runtime value, that's a plain `enum` + runtime `match` — a different tool, already supported.)

**Gist:** A typestate type names its states in a dedicated `state TypeName { … }` block; the `#State`/`#Transition` markers reference those names.

**Story.** Earl models a reservation lifecycle. He wants the set `{ Pending, Confirmed, CheckedIn }` named in one place, tied to `Reservation`, so a typo (`#Transition(Pending -> Confrimed)`) is caught and "is there a dead-end state?" can be asked — without maintaining a list that drifts from the transitions. `state Reservation { … }` is that one place, and it reads as "these are Reservation's states."

**In the wild:**
```jet
// Option B (recommended): a dedicated state-set declaration, tied to the type by name
state Reservation { Pending, Confirmed, CheckedIn }

struct Reservation { guest: String }

impl Reservation {
    #Transition(_ -> Pending)          fn book(guest: String) -> Reservation { return Reservation { guest: guest } }
    #Transition(Pending -> Confirmed)  fn pay(self: ^Reservation) -> Reservation { return self }
    #Transition(Confirmed -> CheckedIn) fn check_in(self: ^Reservation) -> Reservation { return self }
    #State(CheckedIn)                  fn room_key(self) -> String { return "key for {self.guest}" }
}
// `#Transition(Pending -> Confrimed)` → error: Confrimed is not a state of Reservation
// the bounded set also lets the checker warn on a state with no outgoing transition (dead end)
```

**Other languages:** Rust's type-state pattern has no declaration (each state is a marker type; the set is implicit in which `Reservation<S>` impls exist). Statechart libraries (XState/TS, Stateless/C#) declare the whole machine up front for exhaustiveness + diagrams. `state TypeName { … }` is the lightweight middle: one named set tied to the type, no separate machine table.

**Tradeoffs:** (subagent-reviewed)

| Option | How states are named | New keyword | Exhaustiveness (typo / dead-end) | Reads as |
|---|---|---|---|---|
| A loose `tag`s (shipped) | each state a standalone `tag`, set implicit in the markers | none | only the normal undefined-tag error | scattered; no "these are the states" home |
| B `state Reservation { … }` block (recommended) | one declaration, tied to the type by name | one (`state`) | yes — bounded set; flags typo'd + dead-end states | "Reservation's states are …"; cohesive with `#State`/`#Transition` |

- **Option A — loose `tag`s define the set (shipped today).**
```jet
tag Pending {}                       // three independent tags; the set is whatever the markers happen to mention
tag Confirmed {}
tag CheckedIn {}
```

- **Option B — a `state TypeName { … }` declaration. (recommended — owner-directed)**
```jet
state Reservation { Pending, Confirmed, CheckedIn }   // one named, grouped, typo-checked, type-scoped set
// `state` declares · `#State(X)` requires · `#Transition(A -> B)` moves — one family
```

**Recommendation:** **B** — `state Reservation { … }`. It gives the grouping, typo-catching, and dead-end detection the card wanted; ties the set to its type by name; erases by definition (pure compile-time, no runtime discriminant); and forms one coherent `state` / `#State` / `#Transition` family rather than overloading `enum`. The one new keyword is small and purpose-fit. The shipped feature uses loose tags (A); moving to `state { }` is a contained change — add the `state` declaration (parse + register the bounded set, in the declaration family alongside `tag`/`struct`), and re-point the `#State`/`#Transition` marker resolver at it (the E0150 gate, dataflow, and erasure are unchanged). Say ratify and I'll build it.

**Owner Q (D-STATE-DECL):** ratify **B** = `state TypeName { … }`? (One sub-choice if yes: should an unreachable / dead-end state be a hard error or a warning — I'd default it to a warning so a partial machine still compiles during development.)

---

**Also still open upstream (named, not blocking the value-prefix core):**
D-QUAL4 — plain value-tag *type-position* spelling (`#Tag Type` vs `Type #Tag`). The
typestate core above never writes a state in a type position (states ride the value and
the markers), so it does not depend on D-QUAL4. If the owner later wants a state written
in a signature type (`fn f(r: Reservation #Pending)`), that rides D-QUAL4.

---

### D-JIT2 — where the Cranelift dependency physically lives (rec A — board card c139)

_Rides D-JITDEP1 (Cranelift already approved, runtime-side; I6 holds). This decides ONLY where the crate physically sits so I6 ("zero external crates in the compiler") stays machine-checkable, not the runtime design (settled: production is AOT, the JIT is the resident dev-loop tier)._

**Gist:** D-JITDEP1 approved the Cranelift JIT dep "never in compiler `Source/`," but the repo is one crate — so decide whether the wall is a new `jet-jit` workspace crate (A), a cfg-gated I6 carve-out in the one crate (B), or an out-of-tree optional component (C).

**Story.** Walter maintains the Jet toolchain and runs the I6 check in CI: zero external crates in the compiler. The team wants Cranelift to power fast `jet serve` hot-swap. Walter needs to know where the Cranelift dependency physically goes so that a `cargo tree` (or a lockfile grep) on the compiler still shows nothing external — that the I6 wall is something CI can *prove*, not a comment everyone promises to honor.

**In the wild:**
```shell
# The I6 guarantee Walter wants to keep machine-checkable, with Cranelift in the tree somewhere:
$ cargo tree -p jet | grep -i cranelift        # must print NOTHING for I6 to hold
$ jet serve                                     # but this dev loop needs the Cranelift JIT
  watching src/ … hot-swap on save (Cranelift tier-1)
```

**Other languages:** Rust's own toolchain isolates codegen backends as separate crates (`rustc_codegen_cranelift`, `rustc_codegen_gcc`) behind a stable interface, swapped in at build/run time — the workspace-member model (A). LLVM-based toolchains link the backend as an external library (closer to B/C). Go ships a single self-contained toolchain with its backend in-tree (no external dep to wall off — not Jet's situation, since Cranelift is third-party). The cross-toolchain norm for a *third-party* backend is a separate crate behind a stable seam (A).

**Tradeoffs:** (subagent-reviewed)

| Option | Where Cranelift lives | I6 for the compiler crate | Default `jet serve` UX | Fits frozen c140/c141 successors |
|---|---|---|---|---|
| A workspace member `jet-jit/` | own crate, behind `--features jit` | enforced — lockfile-provable | works out of the box | yes — successors are more members behind the same seam |
| B cfg-gated carve-out in the one crate | `Source/Jit/`, off-by-default `jit` feature | documented exception, not enforced | works out of the box | yes, but each successor widens the carve-out |
| C out-of-tree optional component | separate installed component | untouched (strongest isolation) | degraded — JIT not present by default | yes, but each successor is another install |

**The problem.** D-JITDEP1 approved Cranelift as a runtime-side dep and said it must
"never [be] in compiler `Source/`" — I6 holds. But the analogy it draws is to D-REGEX1,
and that analogy does not fit cleanly: the `regex` dep lives inside a **Jet Core
sub-library** (Jet-language code that compiles to Rust and pulls the crate as a normal
package dep). The Cranelift JIT is **not** Jet-language code — it is Rust that consumes
the compiler's TIR, holds executable memory, and satisfies the `JitBackend` trait
(`Source/JitBackend.rs`). It is part of the toolchain, not a shipped Core package.

And there is a hard structural fact: **the repo is a single crate today** (one root
`Cargo.toml`, `name = "jet"`, no `[workspace]`, no members). `Source/JitBackend.rs` is
`pub mod JitBackend` in that one crate. So "Cranelift not in `Source/`" cannot mean
"a different file in the same crate" — a dep in `Cargo.toml` is reachable from every
file in the crate; the I6 wall would be by-convention only, not enforced. We need the
owner to pick how the wall is actually drawn.

The runtime semantics are settled (D-JITDEP1): production is AOT; the JIT is the
resident dev-loop tier only. This ballot is **only** about *where the crate dependency
sits* so that I6 ("zero external crates in the compiler") stays true and enforceable.

---

- **Option A — new workspace member `jet-jit/` (own crate, owns the Cranelift dep). (recommended)**
Convert the repo to a Cargo workspace. `Source/` becomes the `jet` crate and stays
dep-free. A new sibling crate (e.g. `jit/`, crate name `jet-jit`) depends on Cranelift
and on `jet` (for the `JitBackend` trait + a TIR view), and implements `CraneliftBackend`.
The `jet` binary depends on `jet-jit` only behind a `--features jit` flag, so a default
`cargo build` of the compiler still pulls zero crates.

```
Cargo.toml            # [workspace] members = ["jet", "jit"]  (or keep Source/ as crate "jet")
Source/               # crate "jet" — I6: std-only, ENFORCED (no cranelift in its deps)
  JitBackend.rs       #   trait seam (unchanged) + a TIR accessor for backends
jit/
  Cargo.toml          # depends on cranelift-*, and on jet
  src/lib.rs          # impl JitBackend for CraneliftBackend
```

- I6 stays literally true and *machine-checkable*: `Source/`/crate `jet` has no
  Cranelift in its dependency tree; a CI grep on the `jet` crate's lockfile proves it.
- Matches R7 ("backend is swappable; nothing outside codegen/driver knows the target")
  and the existing seam design (a second `impl JitBackend` with zero caller churn).
- The frozen successors (c140 bytecode VM, c141 native JIT) become *additional*
  workspace members swapped in behind the same trait — no `Source/` churn ever.
- Cost: a one-time workspace conversion (jetpack bin, zed wasm crate, test layout must
  still build). The TIR must expose a stable, crate-visible accessor for `jet-jit` to
  read — today TIR types are `pub(crate)` inside `jet`, so a small surfaced view is new
  work.
- Consequence to weigh: this is the only option that keeps I6 an *enforced* invariant
  rather than a documented promise.

- **Option B — explicit, scoped I6 carve-out; Cranelift dep in the one crate behind a feature.**
Keep the single crate. Add `cranelift-*` to `Cargo.toml` under an off-by-default
`jit` feature; `CraneliftBackend` lives in `Source/` (e.g. `Source/Jit/Cranelift.rs`)
gated `#[cfg(feature = "jit")]`. Amend I6's text to name a standing, owner-signed
runtime-tier exception (exactly as D-REGEX1 carved one for `regex`).

```shell
$ cargo build                          # default: no jit feature → zero external crates
$ cargo build --features jit           # pulls cranelift-* into the one `jet` crate
$ cargo tree -p jet | grep cranelift   # now prints cranelift — I6 true only by convention/feature audit
```

- Smallest diff; no workspace conversion; TIR stays `pub(crate)` and is reached directly.
- The JIT backend sits next to the TIR it lowers — arguably the most natural place.
- Cost: I6 stops being literally true for the compiler crate. The wall is "off by
  default + cfg-gated," enforced by convention and a feature audit, not by crate
  boundaries. Every future "can this crate go in `Source/`?" question now points at a
  precedent that says yes-with-a-flag. This is the carve-out the prompt names as
  option (b), and it is a real softening of I6.

- **Option C — `jet-jit` as an out-of-tree optional toolchain component (plugin).**
`jet-jit` is its own crate (as in A) but NOT a workspace member built by default — it is
an optional component the user installs (`jetpack add-component jit` / a separate build)
and the `jet` binary loads/links only when present. I6 is untouched; the compiler never
even references Cranelift.

```shell
$ jet serve
  note: JIT component not installed — `jet serve` runs on the tier-0 interpreter
        install the fast path: jetpack add-component jit
$ jetpack add-component jit            # separate build, version-skew managed against the jet binary
$ jet serve                            # now Cranelift-backed
```

- Strongest I6 isolation; the JIT is opt-in toolchain surface, philosophically "expert
  tier."
- Cost: most plumbing — a component/loading story, version-skew management between the
  `jet` binary and the JIT component, and a worse default dev-loop UX (the headline
  feature of D-JITDEP1 is *fast `jet serve`*, which a non-default component undercuts).
- Likely over-engineered for a dev-loop accelerator that should "just work."

---

**Recommendation:** **A** — a `jet-jit/` workspace member is the only option that keeps I6 *machine-checkable* (a CI grep of the `jet` crate's lockfile proves zero external crates), matches R7 (swappable backend behind a stable seam) and the existing `JitBackend` design, and lets the frozen successors (c140 bytecode VM, c141 native JIT) slot in as more members behind the same trait with zero `Source/` churn. Its one cost is a one-time workspace conversion — vetted as low-disruption: the `jet` and `jetpack` bins stay in-crate, the `editors/zed/wasm-src` crate is standalone and untouched, and only a small stable TIR accessor (TIR types are `pub(crate)` today) is new work. **B** is the lightest diff but converts I6 from an enforced wall to a documented promise (every future "can this dep go in the compiler?" then cites it). **C** maximizes isolation but makes the JIT a non-default component, undercutting D-JITDEP1's whole point (fast `jet serve` that just works).

**Owner Q (D-JIT2):** confirm **A** (workspace member — I6 stays provable), or pick **B** if you'd rather keep one crate and accept I6 as a documented, feature-gated exception.

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

