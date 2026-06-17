# Decision ballots — owner input (open queue)

**Ratified decisions live only in [`syntax-decisions.md`](syntax-decisions.md).**
This file lists ballots that still need an owner pick. When you decide: move
the row to `syntax-decisions.md`, remove it here, and update `src/syntax.rs` /
parser as needed.

**HTML form (optional):** [`decision-ballots.html`](decision-ballots.html)

**Implementation plans:** [`docs/plans/`](../plans/)

---

## Open

*(none)*

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
