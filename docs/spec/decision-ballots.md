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

**The queue is empty.** No decisions are awaiting the owner. New decisions get a
row here (with worked examples per the house rule above) and leave the instant
they are ratified.

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
- **Sidequest language features (D-ILE1, D-BIND1, D-LABEL1, S6-R, D-IF1, D-IF2)**
  — ratified 2026-06-18; recorded in `syntax-decisions.md` and their sidequest
  plans (`docs/plans/sidequests/`). D-IF2 settled D-IF1's multi-arm `if` surface
  (`else` catch-all, braceless arm bodies, structural bare-value/condition mix).
