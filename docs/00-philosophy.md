# 00 — Philosophy

This file is the constitution. When two goals collide, the higher-ranked one
wins. The accepted currency for having both is **implementation effort** —
the owner has explicitly chosen to spend more build time rather than trade
away ease of use or performance.

## Ranked priorities

1. **Memory & type safety.** Never traded away, never configurable. 
2. **Beginner experience.** Learnability and diagnostics are the product.
   If a feature can't be explained in two sentences to someone writing
   their first compiled language, it needs a redesign or a tier (see C1).
3. **Runtime performance.** Zero-cost defaults via the Rust backend. We
   never add runtime overhead to buy simplicity (no GC, no hidden boxing).
4. **Language smallness.** One obvious way. Features fight to get in;
   the default answer to "should we add X?" is no, with a great error
   message and a workaround instead (the simplicity ratchet, invariant I8).
5. **Implementation simplicity & compile speed.** Matters, loses to 1–4.
6. **Rust ecosystem interop.** FFI-tier, post-v1 (milestone M7). Not a
   v1 goal; see conflict C2.

Tie-break rule: when a decision trades rank N against rank M, the smaller
number wins. When it trades effort against anything, effort loses.

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

Async/await; user-defined macros; inheritance; operator overloading;
lifetime syntax; multiple string types; null (absence will be `Option` in
M3+); global mutable state; a self-hosted compiler; `no_std` / sub-std
binary sizes; a required project structure or package manifest.

## Audience (provisional — owner to ratify)

Someone writing their first compiled language: CLI tools, small services,
learning projects. Not (yet): kernels, embedded, async network servers.

**Owner direction (2026-06-11):** the bar for v1.0 rises to a second
audience — experienced developers who would otherwise reach for Go, Zig,
C, or Rust for small tools, and who should *prefer* Jet: minimal
friction by default, control when wanted, with performance and safety
enforced underneath by the Rust backend and the ownership model. The
roadmap (docs/05) reflects this; the ranked priorities above do not
change — beginner experience still outranks everything but safety.

**Owner direction (2026-06-12):** Jet's identity is the **best hybrid
language** — learn from every modern and long-standing language, adopt
the best non-conflicting parts, with exceptional readability, ergonomic
defaults, and a batteries-included experience that is approachable for
beginners and loved by experts. Consequences, recorded here so plans
stop inheriting the older, smaller vision:

- **Long-horizon targets now include embedded systems and kernels.**
  The "Not (yet)" audience line below stands for v1.x, but post-v1
  work must not paint us into a corner that forecloses freestanding
  targets. The Rust backend (`no_std`/`core`) is the enabling path.
- **An expert low-level tier is required** — true C/C++/Rust/Zig-class
  control (raw memory, layout, volatile, allocators), gated so it never
  confuses beginners and never slows programs that don't use it.
  Gating ratified as S58 (`std/mem` import + `unsafe` blocks,
  Zig-style allocators). Onboarding materials never mention it until
  needed.
- **C FFI is a needed future addition** (S59 ratified deferred to v2).
  Rust FFI (M7) ships first; the C ABI story follows in v2.
- **Purity is a product feature, not just a comptime detail.** `pure fn`
  (S60 ratified) marks functions the compiler verifies as pure so Jet
  can eventually replace the Nix language for declarative configuration
  via `jet eval --pure` (see docs/jetpack.md, unratified).
- **Go's territory (networking etc.) is standard-library scope**, built
  out post-v1 — never core-language scope.
- Invariant **I1 will need a measured amendment** when the expert tier
  lands: user-facing Jet stays safe-by-default, but vetted, audited
  low-level helpers in generated code (volatile/MMIO and similar)
  cannot be expressed without Rust `unsafe` internally. The amendment
  is owner-gated and not yet drafted.

**Status note (2026-06-12):** docs/jetpack.md and docs/jetos.md were
developed separately, are **not ratified**, contain **no decided
syntax or semantics**, and conflict with docs/plans/m12-packages.md.
They must be reconciled with M12 before any package-manager work starts.
