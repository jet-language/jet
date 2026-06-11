# 00 — Philosophy

This file is the constitution. When two goals collide, the higher-ranked one
wins. The accepted currency for having both is **implementation effort** —
the owner has explicitly chosen to spend more build time rather than trade
away ease of use or performance.

## Ranked priorities

1. **Memory & type safety.** Never traded away, never configurable. No
   `unsafe` exists in the language or in generated code (v1).
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
(provisional: `read` / `write` / `take`, decision S10). References cannot
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
