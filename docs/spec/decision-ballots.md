# Decision ballots — open owner queue

Every decision waiting on the owner, and **nothing else**. The instant a
decision is ratified it leaves this file: delete the row, implement it, and
build it into its destination doc/code. No "recently ratified" section, no
tables of decided history — that clutter is what this file exists to avoid.
The ratified record lives in the decision log in
[`syntax-decisions.md`](syntax-decisions.md).

**House rule for whoever edits this file:** every decision below carries a
worked, user-story example for each option. The owner decides from concrete
artifacts — what a real person types, sees, and hits as an error — not from
abstract option tables. A bare ballot is not ready to show him. If you add a
decision, add its examples in the same edit.

---

## No open ballots

The Jet language module system (D-MOD1–4) was ratified 2026-06-18 (Rust's model
with `module`/`.` surface swaps and Rust-exact `pub use` re-export) and
implemented; see the decision log in [`syntax-decisions.md`](syntax-decisions.md)
and [`../plans/modules.md`](../plans/modules.md). Nothing else is currently
waiting on the owner.

---

## Parked — not open ballots

Kept out of the queue deliberately so the owner sees only live decisions.

- **Loop unification (amends S19)** — decided: `loop` is the one form;
  `while`/`for` become teaching errors. No longer a decision — it is an
  implementation task tracked in `docs/plans/sidequests/s19-amend-loop-unification.md`.
- **jetos config surface (former D-OS2…D-OS6) and platform (D-NX1…D-NX6)** —
  **deferred to post-Epoch-3.** jetos is research-track until then; do not
  ratify its surface syntax during Epoch 2/3. Context lives in
  `docs/plans/jetpack-jetos/`.
- **Epoch-2 milestone ballots (D-REF2, D-LIB1/2, D-JSON1, D-IO2, D-PKGS4,
  D-TEST1, D-TOOL2/5, D-CROSS2/3) and all REPL refinements (D-REPL*)** — ratified
  2026-06-16/17; recorded in `syntax-decisions.md` and the relevant milestone
  plans. They left this queue per the house rule.
