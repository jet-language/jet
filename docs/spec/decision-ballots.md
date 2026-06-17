# Decision ballots — owner input (open queue)

**Ratified decisions live only in [`syntax-decisions.md`](syntax-decisions.md).**
This file lists ballots that still need an owner pick. When you decide: move
the row to `syntax-decisions.md`, remove it here, and update `src/syntax.rs` /
parser as needed.

**HTML form (optional):** [`decision-ballots.html`](decision-ballots.html)

**Implementation plans:** [`docs/plans/`](../plans/)

---

## Open

### D-DEV4 — what does `jet dev` mean? (blocks E2-M4)

Two ratified directions both claim the command `jet dev`:

- **Shipped (jetpack track):** `jet dev` → `jetpack enter` — enter the project's
  dev shell built from `env.jet`. Documented in `unified-ecosystem.md` §2.2,
  `jetpack-jetos/README.md`, and implemented in `src/main.rs`.
- **E2-M4 plan:** `jet dev <file>` → a long-running **watch + interpret** loop
  (re-check/re-run on save, sub-200ms feedback, interpreter-backed).

These are different products sharing one name. The M4 interpreter *engine*
(whole-program tree-walker, differential battery, E2201/E2202 boundary
diagnostics, latency harness) is decision-independent; only the **command
surface** is blocked on this pick. E2-M4 is paused on D-DEV4 while other
unblocked milestones proceed.

| Opt | Shape | Worked example |
|---|---|---|
| **A** (rec) | Disambiguate by argument: `jet dev` (no file) enters the dev shell; `jet dev <file.jet>` watches+interprets that file. | `jet dev` → drops into the env.jet shell · `jet dev app.jet` → `watching app.jet … (Ctrl-C to stop)` then live checks/output on save |
| B | Give the watch loop its own verb — `jet watch <file>` — and leave `jet dev` = enter shell, untouched. | `jet dev` → shell · `jet watch app.jet` → live loop |
| C | Unify: `jet dev` always means the live project loop (watch the entry from `payload.jet`/`env.jet`); entering a raw shell stays `jetpack enter` only. | `jet dev` in a project → watches the project entry · raw shell only via `jetpack enter` |

**Recommendation: A** — argument presence is an unambiguous, discoverable split
(a bare `jet dev` is "I'm working in this project"; `jet dev file.jet` is "watch
this file"), and it leaves every shipped `jet dev` doc/behavior correct. B is the
safest (zero collision) but spends a second verb on what users will reach for as
"dev". C is cleanest long-term but redefines the shipped no-arg `jet dev`.

*(jetos ballots D-OS2…D-OS6 remain open — see below.)*

---

## Recently ratified (2026-06-16)

Recorded in **`syntax-decisions.md`** — do not duplicate here:

- **U11–U18** — jetpack/jetos typed surface (`System`/`Image`/`Service`,
  `jetpack os`, library `use`, inferred constructors)
- **D-CBIND2/3/5/6, D-LL2** — C FFI bind timing/engine/strings/macros and
  `@audit` on `@unsafe`
- **S75/S76** — fan-out `f.[…]` and fixed-size lists `[T#N]`
- **D-REPL1…21** — terminal REPL (E2-M18); see `m18-repl.md`

Worked examples for the jetos config surface remain in
[`docs/plans/jetpack-jetos/unified-ecosystem.md`](../plans/jetpack-jetos/unified-ecosystem.md)
and [`spec.md`](spec.md) (U15/U16 sections).

**Still open (jetos):** `D-OS2…D-OS6` — service/guard/option declaration syntax;
see [`jetos-design.md`](../plans/jetpack-jetos/jetos-design.md) §9.
