# Philosophy

This file is the constitution. When two goals collide, the higher-ranked one
wins. The accepted currency for having both is **implementation effort** —
the owner has explicitly chosen to spend more build time rather than trade
away ease of use or performance.

## The two-facet design

Jet has two audiences sharing one language:

**Beginners** get magic. Batteries included, first-party packages, excellent
tooling, ceremony-free entry. Footguns do not exist in their world — they
are structurally hidden behind explicit opt-in gates, not merely
undocumented. A beginner can build anything quickly, cleanly, and easily
without ever encountering undefined behavior or memory unsafety.

**Experts** can look behind the facade. When control is needed — raw memory,
layout, volatile, custom allocators, unsafe operations — the expert opts in
explicitly, takes responsibility, and gets the full capability. The compiler
does not prevent expert-tier work; it requires the expert to ask for it.

The design principle: *footguns are opt-in, not opt-out.* Beginners never
encounter them. Experts choose them.

## The north star: jack of all trades, master of ALL

Jet's goal is to be the last language you need. Frontend, backend, CLI tools,
networking, embedded, systems programming, scripting, configuration — Jet
should be the *best* tool for all of it. Not adequate; best. This is an
ambitious long-horizon target achieved incrementally. v1 scope is narrower,
but design decisions must not foreclose any domain.

## One mechanical path, flexible structure

There is exactly one way to *perform* each operation in Jet. There is not
exactly one way to *arrange* code. Mechanical uniqueness kills confusion;
structural flexibility lets teams write code that fits their style.

Concretely: a struct's methods can be written inline in its body block, or
defined externally using `StructName::method(...)` — both produce the same
program. The same applies to modules. This is one feature with two entry
points, not two features. The compiler treats them identically. Style guides
and `jet fmt` can enforce a project preference; the language never forces one.

## The one law on compiler-held facts

*(D-FACT-LAW1=B, ratified 2026-08-07, card #1620.)*

The compiler holds facts about values, places, scopes, and the build: a value's
range, a scope's rights, a task's duty, a place's write window, a build setting.
One law governs all of them:

> A fact moves toward safety silently. Every move away from safety is one
> written word, at the site, on the record. At runtime, no fact remains.

Said to a beginner: **Jet learns facts for you, never forgets one behind your
back, and every exception is one written word.**

The safe direction differs per plane, and the law does not. Learning more about
a value is safe. Giving up a right is safe. Discharging a duty is safe. Closing
a write window is safe. Each already-ratified rule is a named instance of this
one law, not a rule of its own:

| Ratified rule | The law, in that plane |
|---|---|
| Knowledge is never lost silently (D-TYPE2-EXACT1) | losing certainty is the away-move |
| Rights only shrink as scope nests (D-AUTHORITY-MODEL1) | gaining power is the away-move |
| Package policy may only tighten safety (D-PACKAGE-POLICY-SCOPE1) | the same law at package scope |
| Safety facts only tighten at every layer (D-CONF-MERGE1) | the same law on the build |
| `x != None` narrows `T?` to `T` for free (D-FLOWTYPE1) | gaining certainty is the free move |
| A bound task handle must be joined or detached (D-CONC-JOIN1) | abandoning a duty needs the word |
| Taint spreads on its own; `#Scrub(Tag)` removes it (D-TAG-SURFACE1) | suspicion is gained free, spelled away |

The words are **tighten** and **loosen** (D-FACT-WORD1=A). A fact tightens; a
gate loosens it. `attenuate`, `conserve`, and law-level `narrow` are retired as
law words. Operation names are untouched: flow narrowing keeps its name, and no
user-typed spelling changes.

The law is enforced, not only written. Every row in the one fact registry states
its safe direction and its gate words, and a drift guard fails a row that states
neither (D-FACT-LAW1=B). Every gate lands in one ledger, read by
`jet inspect gates` (D-FACT-GATE1=A). Every registered fact is read with one
mark, `T.@range`, `f.@effects`, `@build.profile` (D-FACT-READ1=A, amended by
D-ONCE-AT1=D, which supersedes the former prefix `$` spelling while preserving
infix `@` package references).

Two walls stand on purpose. The borrow checker is a **prover**, never a plane:
alias and flow analysis over places cannot be a fold of per-operation rules, so
the engine stays its own, and the facts it publishes register as read-only rows
(D-FACT-OWN1=A). The memory sigils `&`, `^`, and `~` obey the law's spirit —
each grade of power costs one written mark — but they are the prover's surface,
not gates, and they never enter the ledger.

## The corpus law: say it once

*(D-ONCE-LAW1=A, ratified 2026-08-07, card #1656.)*

The fact law's discipline extends to the corpus itself. Every truth — a semantic, a table, a message, a spelling, a decision — has exactly one home. Every other appearance is rendered from that home. A law ships with its guard, or it is prose: every registered truth names its home, its renderers, and a guard test, and an underivable second copy is a build failure. Engines compile one source (D-ONCE-TIER1=A restores full tier parity, superseding D-VERDICT-1254-1). Retired spellings ship their retirement as code: renames auto-rewrite, semantic changes hard-error, and every retirement carries an adoption ratchet (D-ONCE-RETIRE1=C). The full slate of fourteen rulings lives in Tower on card #1656 and renders in `docs/spec/syntax-decisions.md`.

## Ranked priorities

1. **Memory & type safety.** Never traded away, never configurable.
2. **Beginner experience.** Learnability and diagnostics are the product.
   If a feature cannot be explained in two sentences to someone writing
   their first compiled program, it needs a redesign or a tier (see C1).
3. **Runtime performance.** Zero-cost defaults via the Rust backend. No
   runtime overhead to buy simplicity (no GC, no hidden boxing).
4. **One mechanical path.** Exactly one canonical way to do each operation.
   Features fight to get in; the default answer is no, with a great error
   and a workaround instead (the simplicity ratchet, invariant I8).
   Structural flexibility (where code lives, how it is nested or
   externalized) is not constrained by this priority.
5. **Implementation simplicity & compile speed.** Matters, loses to 1–4.
6. **Ecosystem breadth.** The Core library grows to cover all target domains
   post-v1: networking, web, embedded, systems. Each addition is
   first-party and curated, not a fragmented afterthought.

Tie-break rule: when a decision trades rank N against rank M, the smaller
number wins. When it trades effort against anything, effort loses.

## Effort is never a deterrent (owner-directed, absolute)

Implementation difficulty, build time, or "this is a lot of work" must **never**
weigh in a recommendation, a decision, an option ranking, or a choice of scope.
The owner has chosen to put in hard work up front so it pays dividends. A path
being hard is **not** a reason against it — and is sometimes the signal that it
is the *right* path. Never present "easier to build" as an advantage or "harder
to build" as a drawback; weigh only the ranked priorities above (safety,
beginner experience, performance, one path, long-term correctness).

**Do it right the first time.** Build features fully and end-to-end — parser →
sema → TIR → AOT codegen → JIT/dev → interpreter → web (when applicable) →
diagnostics → examples → tests → docs — the first time. Never ship a stub, a
partial slice, an AOT-only path, or a "ratified, milestone-pending" /
"JIT owed later" placeholder with the intent to "come back later," unless the
work is genuinely blocked on an unratified upstream decision (name the gate).
Execution-tier parity is invariant I9: AOT, Cranelift JIT, the interpreter, and
web share one meaning, and that meaning lives once in Prelude/CoreLib. Engines
are dumb adapters that call those functions — they must not re-encode policy,
defaults, or error behavior. Parking a feature in `tests/jit_gaps.txt` is not
done. Prove AOT and default `jet run`. If deopt reaches the surface, interpreter
ambient must call the same Prelude function. Half-building and revisiting is
slower and worse than doing it completely once. "We'll finish it later" is not
a plan; finishing it now is.

## Resolved conflicts (do not relitigate without owner sign-off)

**C1 — Beginner-first vs. borrow checking.** Resolved by *progressive
disclosure*, not by hiding the model. Tier 1 (the whole v1 language):
everything is a value; assignment moves; copies are explicit (`clone`);
functions declare access to their parameters with plain words
(provisional: `mut` / `take`, decision S10). References cannot
be stored in structs or returned from functions in v1 — which is exactly
why **no lifetime syntax exists anywhere**. Tier 2 (post-v1, opt-in):
stored/returned references, traits-or-comptime generics — added only if
real programs demand them, behind explicit syntax. The bet: most programs
live happily in Tier 1.

**C2 — Rust library interop vs. minimal language.** Source-level interop
would make Rust's full type system (traits, lifetimes, async, macros) leak
into ours. Resolved: interop is an FFI boundary (M7), not a language
feature. The standard library bridges to Rust's `Vec`/`HashMap`/etc.
internally; users never see that.

**C3 — Transpiling to Rust vs. owning diagnostics.** Resolved: the front
end owns *all* semantics and *every* user-facing error, including a
complete ownership checker. rustc is a soundness verifier and optimizer.
A rustc error on generated code is an internal compiler error in jet,
never the user's problem (invariant I2).

## Distribution tenets (owner-directed)

- **A file is a complete program.** `jet run foo.jet` needs no manifest,
  no project folder, no config. No ceremony stands between a beginner and
  a running program. A package/multi-file story, if it ever comes, is
  opt-in and never required for the single-file case. (Architecture R9.)
- **Small, self-contained output.** One native binary, with only what the
  program uses linked in (strip + LTO). Honest floor: Rust's std sets a
  low-hundreds-of-KB baseline; we accept it rather than drop to `no_std`
  and lose the friendly runtime priority #2 needs. "Smallest possible"
  (size-over-speed) is an opt-in `--small` profile, not the default,
  because it trades against priority #3. (Architecture R8, decision S15.)

## Non-goals for v1

Async/await; user-defined macros; inheritance;
lifetime syntax; multiple string types; null (absence will be `Option` in
M3+); global mutable state; a self-hosted compiler; `no_std` / sub-std
binary sizes; a required project structure or package manifest.

## Audience

**v1:** Someone writing their first compiled language *or* an experienced
developer who would otherwise reach for Go, Zig, C, or Rust for small
tools. Minimal friction by default, control when wanted, performance and
safety enforced underneath.

**Post-v1:** Every developer, for every workload — kernels, embedded,
async network servers, frontend, configuration. Design decisions in v1
must not paint us into a corner that forecloses any of these targets.

## Owner directions (recorded for plan inheritance)

**2026-06-12:** Jet's identity is the **best hybrid language** — learn
from every modern and long-standing language, adopt the best
non-conflicting parts, with exceptional readability, ergonomic defaults,
and a batteries-included experience that is approachable for beginners and
loved by experts.

- **Long-horizon targets include embedded systems and kernels.** The
  `no_std`/`core` Rust backend is the enabling path.
- **An expert low-level tier is required** — true C/C++/Rust/Zig-class
  control (raw memory, layout, volatile, allocators), gated so it never
  confuses beginners and never slows programs that don't use it.
  Gating ratified as S58 (`core.mem` import + `#Unsafe("reason")` blocks,
  Zig-style allocators). Onboarding materials never mention it until
  needed.
- **C FFI shipped** (S59, Epoch 2 E2-M14): `use c.<lib>` modules,
  `#Bindgen`/`#Extern` overlays, and inline `#FFI(c|cpp|asm)` under
  `#Unsafe`. Rust FFI (M7) shipped first. Remaining C ABI correctness
  work is tracked on #180/#436.
- **Purity is a product feature, not just a comptime detail.** An explicit
  `=[]=>` effect row marks a function as pure (S60, as respelled by
  D-SHAPE8=A). This can eventually let Jet replace Nix for declarative
  configuration through `jet eval --pure` (layer 3 post-v1).
- **Go's territory (networking etc.) is standard-library scope**, built
  out post-v1 — never core-language scope.
- **Invariant I1 was amended by D-LL1** (ratified 2026-06-16). Jet stays
  memory-safe and type-safe by default. Generated Rust `unsafe` may appear only
  inside user-written audited `#Unsafe("reason") { … }` or
  `#Unsafe("reason") fn` regions, or in vetted std/mem internals.

**2026-06-15:** v1 Jet source-library package management is consolidated
in docs/plans/jetpack-jetos/unified-ecosystem.md (§10: D-PM1…8). Public binary/dev-shell package
management is the owner-gated `jetpack` track in
docs/plans/jetpack-jetos/README.md; jetos is Phase 2 on top of jetpack.

**2026-06-17:** Jet's dual-facet identity is formalized: magic-first for
beginners, expert control accessible behind explicit opt-in. The north star
is a jack-of-all-trades, master-of-ALL language — no reason to reach for
another language for any workload. One mechanical path per operation, but
structural flexibility in how code is arranged (inline vs. external definitions).
External inherent methods use `fn Type.method(self)` (D-EXTMETH1).
