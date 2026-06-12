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

## Tally sheet (open only)


| Group                  | IDs             | Needed by | Status |
| ---------------------- | --------------- | --------- | ------ |
| — (deferred)           | S56             | post-1.0  | ☐      |
| 7 Platform             | S51 S54 S53 S52 | M10–M12   | ☐      |


Ratified (see docs/02): Group 1 confirmations; Group 2 — S29–S33; Group 3 —
S34–S36; Group 4 — S37–S42; Group 5 — S43 S44 S49 S50; Group 6 — S26 S28
S45 S48 S46 S47 S55 S57.
