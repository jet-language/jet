# Importing Jai into Jet — report

Status: proposal / decision-development. Nothing here is ratified. All Jet syntax is
illustrative and uses the corrected grammar (no semicolons; `x @= v` / `x := v`; `if` is the
switch; `.{ }` / `T.{ }` inferred construction; `loop` only). The companion
`jai-import-vision.md` shows the same vision as readable files across four personas.

## 0. The proposal in one paragraph

Jai's three importable powers are an **integrated build system**, **compile-time execution**,
and a **compiler reachable from build/user code**. Jai ships **no package manager** — that's
Jetpack's lane, untouched. The cohesive move: Jet already has the two seeds Jai's power grows
from — a real compile-time interpreter (`Source/Comptime/`) and a typed-surface→plan
elaborator (`Source/Jetpack/ModuleEval.rs`, already used for `Env`/`System`/`Image`).
Generalize that one mechanism into a **family of typed surfaces** (`Package`, `Env`, `Build`,
`Workspace`, `System`, `Image`) written in one grammar, with computable fields. That delivers
Jai's build system, your nix-shell/package/workspace/OS surfaces, and a self-host path as
**one substrate**. The safety line that keeps the invariants intact: **generate source and
re-enter the normal checked pipeline; never inject AST mid-compile; keep language comptime
pure.**

---

## 1. Devil's advocate — I argued against my own proposal, then fixed it

Each objection is real. The fix is folded into the vision; the residue becomes an owner
decision in §4.

**O1 — "Typed surfaces in `.jet` quietly resurrect the thing I8 forbids: two ways to mean
one thing."** A `Workspace` surface and a `pkg.jet` `packages:` index both list members. An
`Env` surface and a `Build` surface both carry a `sources:`/profile notion. If two surfaces
can express the same fact, that's the I8 violation, not cohesion.
→ **Fix:** surfaces must *partition* responsibility, never overlap. Exactly one home per fact:
identity/deps live in `Package`; member discovery lives in `Workspace`; shell tooling lives in
`Env`; output options live in `Build`; machine config lives in `System`. The report's job is
to draw those lines so no fact has two homes. Where today's design already double-homes
something (e.g. `sources:` appears in both `env.jet` and `jetpack.toml`), that's a **defect to
collapse**, called out in D2.

**O2 — "Killing `jetpack.toml` for a Jet `Workspace` surface trades a boring, tool-parseable,
stable TOML index for a Turing-complete file that can compute its own member list — and now
*reading the repo layout requires running the compiler*."** Every external tool (CI, code
search, a dependency graph dashboard, `git`-based ownership tooling) could parse TOML trivially;
none of them can evaluate Jet. A computed `members: find(...)` means the set of packages isn't
knowable without execution. At Google scale that's a real regression.
→ **Fix / decision (D2):** the `Workspace` surface must have a **statically-readable normal
form** — the common case (`members: find("./packages")`, a literal `sources` map) is a fixed,
declarative shape a non-evaluating parser can read, *exactly* like the TOML. Computation is
allowed but the resolver emits a **materialized, checked-in `.jet/workspace.lock`** (the
flat, tool-readable member list) so external tooling never needs to run Jet. If you want the
guarantee absolute, restrict `Workspace` fields to declarative values + `find()` only (no
arbitrary code) — a deliberately *weaker* surface than `Build`. This is a genuine fork; see
D2.

**O3 — "One grammar is not one mental model. A `System` surface and a `fn main()` look alike
but obey different rules — fields vs statements, declarative vs imperative — and beginners
will conflate them."** Cohesion of *syntax* can mask divergence of *semantics*.
→ **Fix:** surfaces are visibly typed (`System.{ … }`, `Build.{ … }`) and always live under
`module`/manifest files, never mixed into a `fn` body. The grammar is shared; the *contexts*
are distinct and named. A beginner (Persona 1) never sees a surface until they opt into a
project; the bare-file path stays pure code.

**O4 — "`mono.ranker` pulling one package out of a giant repo sounds clean but hides a
resolution monster: how do you fetch one subtree of a multi-GB repo without cloning it all,
and how does its `deps:` (which may name *more* in-repo siblings) resolve?"** The selective-pull
story is only as good as the resolver behind it.
→ **Fix / decision (D3):** in-repo addressing (`source.package`) resolves through the source's
own `jetpack.toml` index — fetched first, manifest-only (a shallow/sparse fetch of just the
index + the named package's subtree + its transitive in-repo deps). This is the U9 "manifest-
only remote probe" already in the design, generalized to monorepo members. The cost is a
sparse-checkout-capable provider; the payoff is that "pull one" never means "build ten
thousand." Naming the mechanism is D3.

**O5 — "The dot rule `.{ }` replacing bare `{ }` is a syntax change to the *core language*,
not a build/meta feature — it's scope creep riding in on a Jai report."** True. U18 (infer
constructor from expected type via bare `{ … }`) is ratified and implemented; `.{ }` changes
that surface.
→ **Fix:** flagged honestly as a **separate core-syntax decision (D1)**, owner-requested,
with its own migration (U18 `{ }` → `.{ }`, `Type { }` → `T.{ }`). It is *not* bundled with
the build work; it stands or falls on its own. Recorded here because you asked for it and it
must reach the ballot, but sequenced independently.

**O6 — "Pure compile-time execution sounds principled but will frustrate the exact power
users you're courting — the Jai crowd *expects* to read a file or shell out at build time.
You'll lose them at `read_file is not allowed`."**
→ **Fix / decision (D4):** keep language comptime pure (reproducibility is priority-aligned and
non-negotiable for the *language*), but give the **build/manifest layer** a *small, declared,
sandboxed* effect vocabulary — `find()`, `@embed()`, profile/target selection — executed by
the driver, inputs hashed into the lock. That covers the real Jai use cases (embed assets,
discover members, generate tables from checked-in data) without unbounded effects. The
genuinely-unbounded cases (shell out to `git describe`) get an explicit, audited build-step
escape hatch, *not* free rein inside the interpreter. The line between "declared build effect"
and "arbitrary effect" is the decision.

**O7 — "Self-host is post-Epoch-4/5. Most of this can't be built for a long time. Is the
report just vapor?"**
→ **Fix:** separate what's buildable *now* from what's terminal. Buildable in Epoch 3:
factor the Rust compiler into internal library seams (no invariant risk), and extend the
existing `ModuleEval` to compute surface fields. The surfaces themselves land incrementally
behind their already-open ballots (`Build`, then `Workspace`; `System`/`Image` wait on the
open D-OS2–D-OS6). Nothing here demands self-host first; self-host *consumes* the seams later.

**O8 — "Five surfaces is a bigger learning surface than five files, not smaller — you've just
moved the complexity."** A fair hit at the cohesion claim.
→ **Fix / honest concession:** the win is not *fewer things to learn* in absolute terms; it's
**one grammar, one discovery mechanism, one evaluation model**, so learning *one* surface
teaches you how *all* of them are read, typed, merged, and computed. A beginner learns zero
surfaces (Persona 1). A CLI author learns two (Persona 2). Only the OS author meets all six.
The complexity is *staged by need*, which is the actual I8 goal under its new wording.

---

## 2. What Jai gives, mapped to Jet (grounded)

| Jai capability | Jet today | The import |
|---|---|---|
| Build metaprogram, `Build_Options` | CLI flags only (`Source/CmdCompile.rs`) | `Build` surface; fields computed by comptime |
| Workspaces (many targets, one run) | `jetpack.toml` `[packages]` (TOML) | `Workspace` surface (D2) |
| `#run` compile-time exec | `Source/Comptime/` — real interp, pure, fuel-limited, `@embed` | extend to surface fields + source-gen (D4) |
| `#insert` / AST injection | none (no user macros — non-goal) | **generate source, re-check** (never inject AST) |
| Message loop / compiler-as-lib | `Source/lib.rs` coarse entry points | Rust-side internal seams now; user-side = reflection only |
| Reflection / `Type_Info` | derives (`#[Codable]`), S56 planned | keep; this is the v1 metaprogramming ceiling |
| implicit `context` (allocator) | — | import as the expert allocator-context (§3 of vision) |
| SOA/AOS | `#layout(columnar)` already shipped | already have it |
| package manager | Jetpack (providers, lock, store) — **ahead of Jai** | Jai has nothing to teach here |

---

## 3. The safety line (why the invariants survive)

- **I3 (codegen is dumb) + R2 (sema is the gatekeeper):** build-time codegen emits **source
  text** that re-enters lexer→parser→sema like any input. Generated code is fully checked. No
  post-typecheck AST injection. *Intact.*
- **Reproducible builds:** language comptime stays pure; build effects are a declared, hashed
  vocabulary (D4). *Intact.*
- **"No user macros" non-goal:** user-side metaprogramming is read-only reflection in v1; the
  Jai message loop / macros are deferred past self-host and need an explicit reversal to open.
  *Honored.*
- **I6 (no external crates in the compiler):** factoring `Source/` into *internal* crates is
  allowed and helps self-host. *Intact.*
- **R9 (single file is a whole program):** every surface is optional; a bare `.jet` runs.
  *Intact.*
- **I8 (new wording):** surfaces partition facts (O1 fix) — one canonical home per semantic
  job; flexible organization stays free. *Aligned.*

---

## 4. Decisions for the owner

Each: question, options, recommendation. Develop the chosen ones into ballot cards with worked
examples (per the syntax protocol) before any code.

**D1 — The dot rule (core-syntax change, owner-requested).**
Replace U18 bare-`{ }` inferred construction with `.{ }` (inferred) and `T.{ }` (explicit);
enums match identically (`if x == { .A -> … }`, value `.A` where the type is known).
(a) Replace U18 entirely — one inferred-construction spelling. **(recommend; owner-stated
intent)** (b) Coexist with bare `{ }` (violates new I8 — two spellings). (c) Keep U18, no dot.
*Note:* this is independent of the build work; sequence on its own. Migration: `{ }`→`.{ }`,
`Type { }`→`T.{ }` across examples/snapshots/formatter.

**D2 — Kill `jetpack.toml`; make `Workspace` a Jet surface — and how static must it stay?**
(a) `Workspace` surface, restricted to declarative values + `find()`, plus a materialized
`.jet/workspace.lock` for external tooling. **(recommend — one grammar, O2 addressed)**
(b) `Workspace` surface, fully computable (max power, weakest external-tool story).
(c) Keep `jetpack.toml` as TOML (two languages persist; O1 double-homing of `sources:` stays).
*Frame:* (a) gets one-grammar cohesion while guaranteeing CI/code-search never run the
compiler. The `sources:` double-home (env.jet vs jetpack.toml) collapses into `Workspace`.

**D3 — In-monorepo package addressing.**
Confirm `source.package` (e.g. `mono.ranker`, `infra/logging`) as the one way a package
addresses a sibling or a member of a remote monorepo, resolved via the source's `jetpack.toml`
index with a manifest-only sparse fetch (U9 generalized).
(a) Adopt as specified. **(recommend)** (b) Require every shared package to be its own repo
(no in-repo addressing — kills the monorepo story). *Frame:* (a) makes "pull just the one I
want" the default at every scale; needs a sparse-checkout-capable provider.

**D4 — Compile-time effect boundary (the crux) — three-tier model (owner-approved direction).**
The boundary is not "pure vs effectful" but **reproducible vs ambient**, in three tiers that
realize the new I8 (magic default, expert door, footgun never default):

- **Tier 0 — pure computation.** No I/O. Always on, invisible. `comptime T = build_table()`.
- **Tier 1 — reproducible effects (the batteries).** A *curated* set of build builtins that
  touch the world but **content-hash their input into `.jet/lock`**, so the build stays
  bit-reproducible. No gate. `@embed("f.csv")`, `find("./packages")`,
  `fetch(url, sha256: "…")` (Nix fixed-output trick: network allowed because the result is
  pinned). Covers ~90% of real Jai build-time I/O; beginners use it without knowing it's an
  "effect."
- **Tier 2 — ambient effects (the expert door).** Genuinely non-deterministic: `exec`/shell,
  `git describe`, wall clock, unpinned fetch, `$HOME`. Gated by an audited marker that
  **parallels `#Unsafe`** (reuses the existing audited-gate + effect-tag machinery — no new
  system): `#Impure("reason") comptime VERSION = exec("git", ["describe"]).stdout`. The gate
  forces a reason, records the impurity in the lock (so `jetpack` warns "not reproducible
  offline"), and is never the default — a bare ambient call is a teaching error pointing at
  Tier 1.

Sub-decision **D4.1 — CI posture for Tier 2:** (a) `#Impure` hard-errors unless `--allow-impure`
is passed, so CI is hermetic by default and the expert opens it deliberately. **(recommend)**
(b) `#Impure` only warns + records. *Frame:* (a) matches Google-scale hermetic-build
expectations; (b) is lighter but lets non-reproducibility into CI silently.

Rejected: pure-only (Tier 0 alone — denies the expert, fails I8); Jai-style inline effectful
`#run` with no tiers (loses reproducibility by default — footgun as default, fails I8).

**D5 — Build-time codegen model.**
(a) Generate **source**, re-enter the checked pipeline. **(recommend — I3/R2 intact)**
(b) Jai-style AST injection / `#insert` (breaks I3). *Frame:* (a) means generated code is
sema-checked like any input.

**D6 — User-facing metaprogramming depth (v1 ceiling).**
(a) Reflection / derives only (no AST rewrite, no macros). **(recommend for v1)**
(b) Read-only observation (a pass may *reject* code, not rewrite). (c) Full Jai (rewrite +
macros) — reverses the no-macros non-goal; post-self-host. *Frame:* (c) is Jai's crown jewel
and the biggest invariant fight; defer.

**D7 — Factor `Source/` into internal library seams now (lexer/parser/sema/codegen/comptime)?**
(a) Yes, schedule in Epoch 3 — serves tooling, the build driver, and the eventual self-host;
I6-safe (internal crates). **(recommend)** (b) Defer to the self-host project.

**D8 — Build profiles: named-and-flag-selected vs ambient-environment-read?**
(a) Named profiles in `Build`, selected by `--profile`/`--release` flag. **(recommend —
reproducible; Persona 2/4)** (b) Read `env("RELEASE")` at build time (ambient, non-hermetic).
*Frame:* (a) keeps two builders from silently diverging — the Google-scale requirement.

---

## 5. Sequencing (grounded in the roadmap)

- **Epoch 3, no invariant risk:** D7 (internal seams). Extend `ModuleEval` to compute surface
  fields (D4a/D5a foundation). Settle D1 (dot rule) on its own track.
- **Epoch 3 → 4:** `Build` surface (D8a), then `Workspace` surface + `.jet/workspace.lock`,
  retiring `jetpack.toml` (D2a). In-repo addressing + sparse provider (D3a). Reflection stays
  the metaprogramming ceiling (D6a).
- **Post-Epoch-3, behind open ballots:** `System`/`Image` surfaces join the family *after*
  D-OS2–D-OS6 resolve — do not let build-unification pre-empt them.
- **Post-Epoch-4/5 / `jet-bootstrap`:** the port consumes the D7 seams; the compiler's own
  build is a `Build` surface. Reopen D6 (macros/message loop) only on an explicit non-goal
  reversal.

## 6. Blockers & resolutions

| Blocker | Resolution |
|---|---|
| I3 vs Jai AST injection | D5a: generate source, re-check. |
| Reproducible builds vs effectful `#run` | D4a: pure interp + declared, hashed build effects. |
| "No user macros" non-goal | D6a: reflection only in v1; full meta needs explicit reversal. |
| I6 vs "compiler-as-library" | D7: internal crates; I6 untouched. |
| R9 (single file, no manifest) | every surface optional; bare `.jet` runs. |
| O1: surfaces double-home a fact | partition responsibilities; collapse `sources:` into `Workspace` (D2). |
| O2: computed workspace breaks external tooling | D2a: declarative normal form + materialized lock. |
| O4: "pull one" from a huge repo | D3a: manifest-only sparse fetch via the source index. |
| Self-host is far out | D7 seams + `Build` surface are the bridge; built in the Rust era. |
| Open jetos ballots | sequence `System`/`Image` after D-OS2–D-OS6. |
