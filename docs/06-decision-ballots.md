# 06 — Decision ballots (owner's queue)

Open syntax decisions for M3–M14. **Ratified choices live only in
docs/02-syntax-decisions.md** — when you decide, agents add the row there
and remove it from this file.

Decide one group at a time. A group must be fully decided before its
milestone starts (plans in docs/plans/ are blocked on these IDs).

---

## Group 4 — Collections & strings (decide before M5)

**S37 — List literal.** `[1, 2, 3]` vs `List(1, 2, 3)` vs `{1, 2, 3}`. Keep A.

**S38 — Map literal.**

- A. `["name": 1, "other": 2]`, empty `[:]` (Swift)
- B. `{"name": 1}` braces (Python/JS/JSON)
- C. no literal; `Map.new()` + inserts

Keep A

**S39 — Out-of-bounds behavior.**

- A. `xs[i]` stops the program with a friendly report; `xs.get(i) -> T?`
for safe access (Rust's split)
- B. `xs[i]` returns `T?` always (total safety, unwrap ceremony)
- C. B for maps, A for lists

Keep A

**S40 — Slicing.** `xs[1..3]` inclusive (S22-consistent), copies the
elements (tier 1: no exposed references). Rust: `&xs[1..3]` half-open
borrow. Experts: will note the copy cost — countered by lint L0501 and
honest docs. Beginners: inclusive matches what `1..3` already means in
`for`. → **Inclusive, copying `xs[a..b]`**; same for `s.slice(a..b)`.  
**Note: In mathematical notation, it would be [ or ] for inclusive & ( or) for exclusive. Is it possible to do that?**

**S41 — Strings & `Char`.**

- A. `s.len()` counts characters; `for c in s.chars()`; single-quoted
`'a'` Char literals; NO `s[i]` indexing (error teaches `.chars()`)
- B. Rust model: byte length, `.chars()` iterator, no indexing
- C. UTF-32 strings, O(1) indexing (memory cost)

Keep A.

**S42 — Numbers beyond `Int`/`Float`.**

- A. Stay `Int`(i64)/`Float`(f64) for v1; add `Byte`(u8) in M10 for
binary data; conversions are named methods/constructors:
`n.to_float()`, `f.to_int()`, `Int.parse(s)`; no `as` keyword
- B. full sized-integer menu now (i8…u64) Rust-style
- C. arbitrary-precision Int (Python) — rejected: priority #3

Rust: B + `as`. Experts: occasionally want u32/u64; can wait for
evidence (FFI in M7 may force it — revisit then). Beginners: A.
→ **A.** `as` gets a teaching error; explicit named conversions only.

---

## Group 5 — Tooling & FFI (decide before M6/M7)

**S43 — Test syntax.**

- A. first-class blocks: `test "name" { … }` with `require`/`require_eq`
- B. Rust attribute style `#[test] fn …`
- C. naming convention `fn test_`*

Keep A.

**S44 — The one true format.** 4-space indent, same-line `{`, width 100,
spaces around binary operators. Rust: same but width 100/rustfmt.
Experts: anything consistent; same-line `{` matches every example so
far. Beginners: don't care, fmt does it. → **Ratify the constants** in
the M6 plan; the only real choice is width — **100**.

**S49 — Doc comments.** `/// summary line` above items (Rust), shown by
hover/docs tooling; plain text v1. Alternatives: `##`, docstrings.
→ `**///`** — it degrades gracefully to a normal comment.

**S50 — Calling Rust (FFI).**

- A. `extern rust "crate@version" { fn name(args) -> T = "rust::path"; }`
- B. annotation per function `@rust("rand::random")`
- C. manifest-only mapping, no inline syntax

Rust: n/a (it *is* Rust). Experts: A — explicit block, visible
boundary, version pinned at the declaration. Beginners: won't touch FFI;
A at least reads declaratively. → **A** (pins migrate into jet.toml when
a manifest exists, per M12).

---

## Group 6 — Functions & generics (decide before M8/M9)

**S46 — Lambda syntax.**

- A. `(x) => x * 2` and `(x) => { … }` (JS/C#/TS arrow)
- B. `|x| x * 2` (Rust pipes)
- C. `fn(x) x * 2` anonymous-fn keyword (Zig-ish)

Rust: B. Experts: all three fine; B is Rust-distinctive but widely
called line noise. Beginners: A — the arrow is the single most
recognized lambda form on earth. → **A.** (`=>` is new; `->` stays for
return types/switch arms — distinct on purpose.)

**S47 — Function types & closure captures.**

- A. type is `fn(Int) -> Int`; captures follow M2 rules automatically;
*escaping* closures clone clonables (lint) and require an explicit
`take(name)` prefix for non-clonables: `take(sender) () => …`
- B. Rust's three traits (Fn/FnMut/FnOnce) surfaced to users
- C. capture lists always required (C++)

Rust: B. Experts: B's distinctions are the #1 Rust closure complaint;
A hides them while sema still enforces them. Beginners: A — zero new
concepts until a closure escapes, then one keyword they already know.
→ **A.**

**S45 — Generic syntax.** `fn largest[T: Comparable](xs: List[T]) -> T?`,
`struct Pair[T] { … }`, bounds `[T: A + B]`. Rust: `<T: Ord>` + `where`.
Experts: brackets fine (Go proved it). Beginners: reads as "for any T".
→ **Square brackets, inline bounds, no `where`, no call-site type
arguments** (annotate a binding if inference fails).

**S28 — Traits.** `trait Shape { fn area(self) -> Float; }` +
`impl Shape for Circle { … }`; explicit, not Go-structural. Rust: same
shape. Experts: explicit impls give better errors than structural
matching (Go's interface-satisfaction errors are notoriously late).
Beginners: "a trait is a promise; impl is keeping it". → **Explicit
`trait` + `impl Trait for Type`.** No default bodies/associated types
in v1.

**S48 — Dynamic dispatch.** Writing a trait name as a type
(`List[Shape]`, `fn f(s: Shape)`) = automatic boxing + dynamic dispatch;
`[T: Shape]` = monomorphized. Rust: explicit `dyn`/`Box<dyn>`. Experts:
will ask "where's my `dyn`" once, then enjoy it (this is exactly the
boxing policy M3 uses for recursion). Beginners: never see it.
→ **Auto-dyn, invisible boxing.**

**S26 — Comptime (close-out).** With S28/S45 recommended, generics come
from traits + monomorphization; Zig-style comptime would be a second
metaprogramming system. → **Close S26 as rejected for v1**; revisit
post-1.0 only if trait bounds prove insufficient.

---

## Group 7 — Platform (decide before M10/M11/M12)

**S51 — Std library access.** `import "std/fs" as fs;` reusing S16
machinery (reserved `std/` prefix) vs auto-available globals vs a `std.`
mega-namespace. Rust: `use std::fs`. Experts: explicit imports, grep
friendly. Beginners: one import line, copy-pasteable. → `**import "std/fs" as fs;`** — `print`/`require`/`panic` stay prelude builtins;
everything else is imported.

**S54 — Naming convention.** Enforce snake_case for fn/vars and
PascalCase for types? Rust: yes (warnings). → **Lint (L1001), warning
only, fmt never renames.** One ecosystem-wide style with no fights.

**S53 — Concurrency surface.**

- A. `tasks.spawn(closure) -> Task[T]`, `t.join() -> T`,
`tasks.channel[T]()` with `Sender`/`receive() -> T or Closed`;
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


| Group                   | IDs                     | Needed by | Status |
| ----------------------- | ----------------------- | --------- | ------ |
| 4 Collections & strings | S37 S38 S39 S40 S41 S42 | M5        | ☐      |
| 5 Tooling & FFI         | S43 S44 S49 S50         | M6/M7     | ☐      |
| 6 Functions & generics  | S46 S47 S45 S28 S48 S26 | M8/M9     | ☐      |
| 7 Platform              | S51 S54 S53 S52         | M10–M12   | ☐      |


Ratified (see docs/02): Group 1 confirmations; Group 2 — S29–S33; Group 3 — S34–S36.