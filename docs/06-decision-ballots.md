# 06 — Decision ballots (owner's queue)

Open syntax decisions for M3–M14. **Ratified choices live only in
docs/02-syntax-decisions.md** — when you decide, agents add the row there
and remove it from this file.

Decide one group at a time. A group must be fully decided before its
milestone starts (plans in docs/plans/ are blocked on these IDs).

---

## Group 7 — Platform (decide before M10/M11/M12)

**S51 — Std library access.** `import "std/fs" as fs;` reusing S16
machinery (reserved `std/` prefix) vs auto-available globals vs a `std.`
mega-namespace. Rust: `use std::fs`. Experts: explicit imports, grep
friendly. Beginners: one import line, copy-pasteable. → `**import "std/fs" as fs;`** — `print`/`require`/`panic` stay prelude builtins;
everything else is imported.

**S54 — Naming convention.** Enforce snake_case for fn/vars and
PascalCase for types? Rust: yes (warnings). Casing is **convention, not
syntax** — teams may differ; the compiler never rejects a name for case
alone. → **Lint (L1001), warning only, fmt never renames.** One
ecosystem-wide default style with no fights.

**S53 — Concurrency surface.**

- A. `tasks.spawn(closure) -> Task<T>`, `t.join() -> T`,
`tasks.channel<T>()` with `Sender`/`receive() -> T or Closed`;
no shared mutable state in v1 (ownership rejects it; channels are
the answer)
- B. `go`-style keyword `spawn { … }` fire-and-forget + channels
- C. defer all concurrency past v1

Rust: `thread::spawn` + mpsc (A's shape). Experts: A — structured
(join is `take self`, so leaks are type errors) beats Go's silent
goroutine leaks; no Mutex means no deadlock FAQ in v1. Beginners: A
with the M11 error messages ("the new task might outlive `data`…").
→ **A**, as std functions not keywords (smallness: no new syntax at all).

**S52 — Package manifest.**

- A. `jet.toml` (tiny TOML subset, hand-parsed): `[package]`,
`[dependencies]` (git/path, exact pins), `[rust-dependencies]`;
lockfile `jet.lock`; commands `jet add` / `jet fetch`; registry later
as a static git index
- B. JSON manifest
- C. manifest written in Jet itself (Zig's build.zig direction)

Rust: Cargo.toml. Experts: A — TOML is the settled answer; C is clever
but makes tooling/registry parsing turing-complete. Beginners: A, it's
three lines. → **A.** Single files stay manifest-free forever (R9).

---

## Group 8 — Post-1.0 horizon (owner direction 2026-06-12; decide before
any post-v1 plan is written — no rush, but registered so nothing drifts)

*(S58 ratified 2026-06-12 — see docs/02: `std/mem` discovery gate +
`unsafe` audit gate, Zig-style allocators, sema-gated `&`/`*`.)*

**S59 — C FFI.** Surface for calling C and being called from C.

- A. **`extern c "header-or-lib" { … }` blocks** mirroring S50's
  `extern rust` shape — one FFI idiom, two backends; by-value boundary
  first, pointers only inside the S58 tier.
- B. Auto-binding generation from headers (bindgen-style tool).
- C. Rust-crate detour only (use Rust's C interop via M7; no native
  surface).

Rust: `extern "C"` + bindgen. Zig: `@cImport` translate-at-compile —
beloved, but drags a C parser into the compiler. Experts: A now, B as
tooling later. Beginners: never see it. → **A**, with B as a separate
tool when demand shows. Jet-export (`pub extern c fn`) rides the same
ballot.

**S60 — Pure-function marking.** The Nix-replacement keystone: mark
functions the compiler *verifies* are pure (no IO/FFI/time/random/
global), so whole files can be evaluated deterministically
(`jet eval --pure`, jetpack JP0) and comptime callability is visible in
signatures.

- A. **`pure fn name(…)`** — a checked modifier; purity is part of the
  signature, violations are compile errors naming the impure call path
  (the E0951 machinery already planned for M9.5 comptime).
- B. No marking — purity stays inferred (today's comptime plan);
  `jet eval --pure` checks files wholesale. Zero syntax, but purity is
  invisible at API boundaries and can't be a stable contract.
- C. Effects system (full inference + polymorphism). Researchy;
  conflicts with smallness.

Rust: none (const fn is the nearest cousin). Experts: A — a contract
you can't see is a contract you can't keep. Beginners: A reads as one
plain word. → **A**, post-1.0; M9.5 ships with inference (B) and A
layers on top without breaking anything.

*(S61 ratified 2026-06-12 — see docs/02: optional argument labels,
positional order fixed, trailing default values.)*

*(S62 ratified 2026-06-12 — see docs/02: Kotlin-style trait delegation
`impl Trait using field;`.)*

*(S63 ratified 2026-06-12 — see docs/02: RAII scope-end cleanup as the
one story; `defer` noted as a possible later complement.)*

---

## Tally sheet (open only)


| Group                  | IDs             | Needed by | Status |
| ---------------------- | --------------- | --------- | ------ |
| — (deferred)           | S56             | post-1.0  | ☐      |
| 7 Platform             | S51 S54 S53 S52 | M10–M12   | ☐      |
| 8 Post-1.0 horizon     | S59 S60         | post-1.0  | ☐      |


Ratified (see docs/02): Group 1 confirmations; Group 2 — S29–S33; Group 3 —
S34–S36; Group 4 — S37–S42; Group 5 — S43 S44 S49 S50; Group 6 — S26 S28
S45 S48 S46 S47 S55 S57; Group 8 (partial) — S58 S61 S62 S63.
