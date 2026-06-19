# `print` & friends in the prelude — make IO "just work"

**Status:** Draft plan — needs owner review (2026-06-19)
**Card:** c13

## Problem & why it matters

The card's premise is half-true and worth correcting up front, because it changes
the plan: **`print` already works with no import.** It is an *ambient builtin*,
special-cased in sema at `Source/Sema/CheckerInfer.rs:1405`:

```rust
// `print` is a builtin that doesn't live in scope as an ident — special-case it
if name == Syntax::BUILTIN_PRINT { … }
```

So `print("hi")` works in a bare script and in the REPL today, no `use` line.
Nothing to build there.

The **real** problem is the *asymmetry* around it. `print` is ambient, but its
natural siblings are not:

- `input`, `read_all_input`, `args`, `eprint` live in `core.io` and require
  `use core.io as io;` then `io.input()` (`Source/Sema/CheckerStdlib.rs:1333`).
- `println` isn't a thing at all — it's a *foreign teaching-error*
  (`Syntax::FOREIGN_PRINTLN`, CheckerInfer.rs:2446) that redirects you to `print`.

So a beginner who writes `print("name?")` then `let n = input()` gets an error on
line 2 for a function that feels every bit as built-in as `print`. The model is
inconsistent: one IO primitive is magic-ambient, the rest demand ceremony. That
inconsistency — not a missing `print` — is what makes IO feel like it doesn't
"just work."

A second, structural concern: `print`'s ambient-ness is a hand-rolled
special-case string match, not a real prelude scope. Adding more ambient symbols
the same way multiplies special-cases. If we widen the ambient set, we should
decide whether to *formalize* a prelude rather than bolt on more `if name == …`.

## Prior art (terse)

- **Rust** — `std::prelude` auto-imports a fixed set (`println!`, `Vec`,
  `Option`, `Result`, common traits). `println!` is in; *reading stdin*
  (`io::stdin`) is **not** — you import it. Rust deliberately keeps *output*
  trivial and *input* explicit.
- **Python** — `print` and `input` are *both* builtins, no import. Beginner REPL
  experience is the gold standard precisely because of this symmetry.
- **Go** — nothing ambient; `fmt.Println` always needs `import "fmt"`. Explicit,
  but the famous beginner friction point.
- **Gleam / Roc** — small explicit preludes; IO is a passed-in effect/module, not
  ambient (different safety model).

Two coherent philosophies: Rust's "output ambient, input explicit" and Python's
"both ambient." Jet today is *neither* — it's "output ambient, input ceremonial,"
which is the worst of both: magic where you don't expect it, ceremony where you
do. The decision is which coherent line to pick.

## Proposed design (worked example)

Formalize a small, named **prelude scope** that holds the ambient symbols, and
move `print` into it instead of the special-case. Then decide which IO reads
(if any) join it. The crux question — *does `input()` work without `use
core.io`?* — is D-PRELUDE1.

Recommended target (Python-leaning, beginner-first):

```jet
// no `use` line anywhere
fn main() {
    print("what's your name?")
    let name = input()           // ambient, like print
    print("hello, {name}")
}
```

```shell
$ jet run greet.jet
what's your name?
Ada
hello, Ada
```

REPL — same, no setup:

```shell
$ jet repl
jet> print("hi")
hi
jet> let n = input()
…
```

Everything *else* in `core.io` (`args`, `read_all_input`, `eprint`) stays
qualified behind `use core.io as io;`. The prelude is deliberately tiny: the two
symbols a first program reaches for (`print`, `input`), nothing more. `eprint`
is stderr (an expert/tooling concern), `args` is CLI plumbing, `read_all_input`
is a power-user streaming primitive — none belong in a beginner's empty file.

### Consistency with the module system

The prelude must not become a back door that shadows the `use` system:

- Prelude symbols are **unqualified ambient names** (`print`, `input`), resolved
  *before* user scope, like `print` is today.
- They remain *also* reachable by their qualified path (`io.input`) for users who
  prefer explicit IO — same function, two spellings, no duplicate definition.
- A user-defined `fn input()` **shadows** the prelude one in that scope (prelude
  is the lowest-priority scope), so the prelude can't trap a name. This mirrors
  how Rust's prelude yields to local definitions.
- Reserved-root rules (`E1002`, spec.md ~518) are unaffected — `core`/`jet` stay
  reserved; the prelude doesn't add a new root, it adds ambient leaf names.

### Beginner story

Empty file → `print`/`input` work → the first program is 4 lines with no
boilerplate. When the beginner needs *more* IO (`eprint`, `args`), the error on
`eprint(...)` teaches `use core.io as io;` — a graduated reveal, not a wall.

## Implementation sketch — file-level touchpoints

- **`Source/Sema/CheckerInfer.rs:1405`** — replace the `name ==
  Syntax::BUILTIN_PRINT` special-case with a lookup against a small
  `PRELUDE_IDENTS` set (still ambient, still resolved before user scope). Add
  `input` (and any others D-PRELUDE1 admits) to that set.
- **`Source/Sema/CheckerStdlib.rs:1333`** — `core.io` member list stays; the
  prelude names map to the *same* `jet_std_io_*` helpers (no duplicate codegen).
- **`Source/Prelude/Std.rs`** — `jet_std_io_args`/input helpers already exist
  (line 458 etc.); no new runtime code, just a new resolution path to them.
- **`Source/REPL.rs`** — REPL shares sema, so ambient prelude names work there
  automatically once sema resolves them; confirm no separate REPL scope seeding is
  needed.
- **`Source/Syntax.rs`** — `BUILTIN_PRINT` already exists (I7); add the new
  ambient names as decision-tagged constants if `input` joins (e.g.
  `BUILTIN_INPUT`).
- **`docs/spec/spec.md`** — document the prelude scope and its exact membership
  in one place (single source of truth); update the `print` builtin note
  (spec.md:116) to "prelude" framing.

No codegen change — prelude names lower to the existing `jet_std_io_*` calls
(I3 holds: codegen stays dumb).

## Test plan

- **Ambient resolution:** a fixture using bare `print`/`input` with no `use`
  compiles and runs; golden output (I5). `examples/features/NN_io_prelude.jet`.
- **Shadowing:** a fixture defining `fn input() -> Int` uses the *local* one, not
  the prelude — assert no clash, local wins.
- **Still-qualified siblings:** `eprint(...)` with no `use` still errors and
  teaches `use core.io` — ui snapshot (the graduated-reveal diagnostic).
- **REPL:** scripted `jet repl` session: `input()` resolves without a `use` line.
- **Dual spelling:** `io.input()` (qualified) and `input()` (prelude) produce
  identical lowering (emit-rust diff is empty).
- **Drift guard:** a test asserting the prelude set in sema matches the documented
  membership in spec.md, so the two can't drift (cf. the LSP keyword-drift
  lesson).

## Risks & invariant check

- **I1/I2/I3** — IO already exists and is memory-safe; this is a *name-resolution*
  change, no new runtime, no codegen change, rustc unaffected.
- **I7** — every new ambient keyword/builtin gets a `Source/Syntax.rs` constant +
  decision ID. Gated on D-PRELUDE1 ratification before code.
- **I8** — net simplification: replaces a string special-case with a small,
  named, documented set. Adds at most one ambient name (`input`). No new language
  feature; if anything it makes the existing magic *honest*.
- **Risk — prelude scope creep.** A prelude is a slippery slope: every "obviously
  useful" symbol wants in. Mitigation: D-PRELUDE1 fixes the membership *and the
  rule for changing it* (owner sign-off per addition, like a syntax decision).
- **Risk — shadowing surprise.** If a user's `input` silently shadows the
  prelude, a beginner copying example code could get a confusing result.
  Mitigation: prelude-yields-to-local is the least-surprising rule (matches Rust),
  and a future lint could flag shadowing a prelude name.

## Open decisions

1. **D-PRELUDE1** — which IO symbols are ambient prelude members? (Card below —
   the load-bearing one: *does `input()` work without `use core.io`?*)
2. Formalize a real prelude scope vs. keep extending the hardcoded special-case?
   (Recommendation baked into the plan: formalize, but it's a small refactor, not
   owner-facing syntax — implementer's call unless the owner objects.)

## Proposed decision card(s)

### D-PRELUDE1 — Which IO symbols are ambient (no `use`)? (rec B)

`print` is already ambient. The question is its siblings. Each option shows the
same first program; the difference is whether line 2 needs a `use`.

- **Option A — Output only (Rust-style).** `print` ambient; *everything else*,
  including `input`, stays behind `use core.io`. Consistent with Rust; input is
  explicit.

    ```jet
    use core.io as io;          // ← required just to read a line
    fn main() {
        print("name?")
        let name = io.input()
    }
    ```

- **Option B — `print` + `input` (Python-leaning, recommended).** The two
  symbols a first interactive program needs are ambient; `eprint`, `args`,
  `read_all_input` stay qualified.

    ```jet
    fn main() {
        print("name?")
        let name = input()      // ← just works, like print
    }
    ```

- **Option C — All of `core.io` ambient.** `print`, `input`, `eprint`, `args`,
  `read_all_input` all ambient, no `use core.io` ever.

    ```jet
    fn main() {
        print("name?")
        let name = input()
        eprint("debug")         // stderr, also ambient
        let argv = args()       // CLI args, also ambient
    }
    ```

- **Option D — Status quo (`print` only, special-cased).** Leave it: `print`
  ambient, `input` requires `use`. (Listed for honesty; this is the inconsistency
  the card is trying to fix.)

    ```jet
    use core.io as io;
    fn main() { print("name?"); let n = io.input() }
    ```

**Recommendation: B.** It makes the model *consistent for beginners* (the two
primitives a first program reaches for are both magic) while keeping the prelude
tiny and the expert/tooling IO (`eprint` to stderr, `args` for CLI) explicit
behind `use` — so the namespace doesn't bloat and `core.io` stays meaningful. A
is the principled-minimalist line (Rust's), and is defensible if the owner wants
*reading* to always be explicit; C over-stuffs the global namespace with stderr
and CLI plumbing a beginner never asked for. **Concrete answer to the crux: under
B (and C), `input()` works with no `use core.io`; under A and D it does not.**
