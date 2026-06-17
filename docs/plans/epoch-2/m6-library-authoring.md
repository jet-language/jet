# E2-M6 — Library authoring ergonomics

**Status:** draft — **blocked on D-LIB1…D-LIB3** (Group M6), **D-ERR1…D-ERR2**
(Group 14, error conversion), and **D-FP1** (field punning, Group 16). S61
(labels/defaults) and S62 (delegation) are *ratified*; this milestone decides
their **timing** only.
**Depends on:** E2-M1 (generics), E2-M5 (references in APIs). Unblocks E2-M8/M9
(first-party packages need clean APIs).
**Error codes:** E24xx block (claim in docs/spec/diagnostics.md).

## Goal

Make Jet excellent for authors of reusable libraries, not just users. The proof
is that the first-party ring (E2-M9) and registry packages can expose clean APIs
without boilerplate explosions, and `?` works across module boundaries without
same-error-type contortions.

## Owner decisions — ratify before any code

| ID | Question | Rec | Default if deferred | Ratified |
|---|---|---|---|---|
| D-LIB1 | S61 (labels/defaults) + S62 (delegation) timing | **A** — both in M6 | A | ✅ ratified 2026-06-17 — A: both S61 and S62 ship in M6; they reinforce each other for library ergonomics |
| D-LIB2 | Generics step | **A** — associated types + default method bodies | A | ✅ ratified 2026-06-17 — A: associated types (`type Key`, `type Value` inside the trait) + default method bodies; covers bulk of library needs without higher-kinded complexity |
| D-LIB3 = D-ERR2 | `?` error-conversion shape | **A** — `From`-style **`Fallible`** trait | trait, opt-in | ✅ ratified 2026-06-16 — **`Fallible` trait, `Error` type** |
| D-ERR1 | Grow `Error` carrier (msg + code + source) | **A** | A | — |
| D-FP1 | Struct field punning | **A** — `Source { name, upstream }` | A | — |
| D-FP3 | Core `module name {}` typed declaration | — | — | ✅ ratified 2026-06-16 — A: core `module name {}` typed declaration |
| D-OWN1 | Implicit-clone lint | — | — | ✅ ratified 2026-06-16 — A: keep + strengthen implicit-clone lint |
| D-OWN2 | Ownership mini-examples | — | — | ✅ ratified 2026-06-16 — A: add ownership mini-examples |
| D-OWN3 | `take` suggestion site | — | — | ✅ ratified 2026-06-16 — A: suggest `take` at call site |
| D-JSON1 | JSON decode strictness baseline | — | — | ✅ ratified 2026-06-17 — B: lenient coerce where unambiguous (`"8080"` → `8080`); only error on truly impossible conversions; **implementation must surface coercions** (see owner-todo.md — brainstorm per-decode report or build-output log so magic is legible without breaking) |
| D-JSON2 | Unknown JSON keys | — | — | ✅ ratified 2026-06-16 — A: ignore unknown JSON keys by default, opt-in strict |

## Surface (uses ratified S61/S62 + ballot recs)

```jet
// S61 — optional argument labels + trailing defaults:
fn connect(host: String, port: Int = 5432, tls: Bool = true) -> Conn ? { … }
connect("db.local", tls: false);          // labels catch transposed args

// S62 — trait delegation, no invisible name injection:
struct Logged { inner: Service; }
impl Service using inner;                  // forwards Service methods to `inner`

// D-FP1 — field punning:
return Source { name, upstream, via: "nix" };

// D-ERR2 — `?` converts across error types via the Fallible trait:
impl JsonError: Fallible { fn to_error(self) -> Error { … } }
fn load(path: String) -> Config ? {
    val text = fs.read(path)?;   // FileError -> Error via Fallible
    ok(parse(text)?)             // JsonError -> Error via Fallible
}
```

## Scope

- **Generics v1.5 (D-LIB2):** associated types and default method bodies.
  Re-evaluate trait inheritance and blanket impls only with evidence (I8).
- **Error conversion (D-LIB3/D-ERR2):** `?` across different error types via the
  opt-in **`Fallible`** trait (`impl E: Fallible { fn to_error(self) -> Error }`);
  `String` and std error types convert by default;
  arbitrary unrelated enums do **not** silently collapse.
- **`Error` carrier (D-ERR1):** grow the prelude `Error` to hold message +
  optional code + optional source, replacing today's `String` backing.
- **Labels/defaults (S61) + delegation (S62):** land both, timing per D-LIB1.
- **Field punning (D-FP1):** `Type { name }` when a local matches the field name;
  static field checking, good "missing/misspelled field" errors.
- **API-design lints:** advisory lints for public package surfaces (e.g. boolean
  positional args, leaking internal types). Advisory only — never block a build.
- **Docs/examples** for library API style.

## Diagnostics to register

- **E2401** delegation target lacks a required method ("`inner` does not provide
  `flush`").
- **E2402** `?` error type has no **`Fallible`** path to the function's error type
  (names both types; suggests an `impl Fallible`).
- **E2403** field-pun name not in scope / not a field (with "did you mean").
- **L2401** advisory: public API takes a positional `Bool`; consider a label.

## Examples & tests

- `examples/features/36_library.jet` — a small library using labels, defaults,
  delegation, punning, and cross-type `?`.
- ui fixtures for E2401–E2403 and the L2401 advisory.
- A multi-module example proving `?` conversion across files.

## Out of scope

- Higher-kinded types, trait objects beyond current dynamic dispatch.
- Macros / derive expansion beyond S55 built-ins.
- Implicit any→any error conversion (rejected as too vague, D-ERR2 option C).
- Operator overloading beyond ratified built-in traits.

## Exit criteria

- First-party packages can expose clean APIs without boilerplate explosions.
- `?` in multi-module programs works without same-error-type contortions.
- Argument labels catch transposed boolean/string arguments (tested).
- Delegation removes real repeated forwarding without invisible name injection.
- `nix develop -c cargo test` green; new diagnostics have snapshots + `jet explain`.
