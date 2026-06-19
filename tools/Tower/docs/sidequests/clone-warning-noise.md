# L0201 implicit-clone warning noise

**Status:** Draft plan — needs owner review (2026-06-19)
**Card:** c12

## Problem & why it matters

L0201 fires whenever a value is passed by read to a parameter that wants to take
ownership, on a cloneable type, inserting an implicit `.clone()` and warning the
user (spec.md:156; diagnostics.md:148). The message:

> implicit clone of `x`; write `move x` to transfer ownership or `.clone()` to
> silence this warning

The complaint: it fires on **every** `String` passed to a stdlib function that
stores it — e.g. building a struct, pushing into a collection, constructing a
JSON value. For idiomatic code that passes the same name into a few constructors,
this is a wall of warnings the user can do nothing useful about: they *want* the
value after the call, so `move` would be wrong, and `.clone()` just silences a
warning the compiler could have stayed quiet about. High false-positive rate,
low signal — classic lint noise that trains users to ignore lints.

The card says **"revisit post-v1 with evidence,"** and that framing is correct.
This plan lays out (a) what a real fix would cost, (b) a smarter model if we fund
it, and (c) why **defer** is a legitimate — possibly the recommended — answer
under I8.

## The mechanics (verified)

L0201 is **not** one site. It fires from four, each with the identical message
string:

- `Source/Sema/CheckerItems.rs:185` (named-arg / method calls)
- `Source/Sema/CheckerOwnership.rs:515` (plain calls, ownership pass)
- `Source/Sema/CheckerInfer.rs:2890` (inference pass)
- `Source/Sema/CheckerStdlib.rs:817` (stdlib calls; a variant at :819 covers the
  JSON "value is borrowed, copied into the JSON value" wording)

Any real change to the *firing condition* must be a shared helper threaded
through all four — otherwise the four drift. That alone roughly quadruples the
implementation and test surface and is the strongest argument for waiting until
there's evidence the change pays for itself.

The trigger is `(param_conv = Move, arg_conv = Read)` on a `is_cloneable` type
with an `Ident` argument (CheckerItems.rs:181–185). The clone is *correct* — the
program compiles and is safe. L0201 is purely advisory.

## Prior art (terse)

- **Clippy noise tuning** — clippy's history is largely *lint-level triage*:
  pedantic/nursery lints are off by default; noisy lints get `allow`-by-default or
  are gated behind `--W clippy::pedantic`. The lesson: when a lint is right "in
  principle" but noisy "in practice," the fix is usually **changing when it
  fires / its default level**, not building deeper analysis.
- **Rust's own clone story** — rustc does *not* warn on implicit clones at all;
  `clippy::redundant_clone` warns only when a clone is provably *dead* (the value
  is never used after). That's the key insight: warn only when the clone is
  **wasteful** (value unused after), not when it's merely *present*.
- **Swift/C++ copy elision** — copies the compiler can prove unnecessary are
  elided silently; the user is never warned about a copy they can't avoid.

The cross-language consensus: **don't warn on a copy the user can't or shouldn't
remove.** Warn only when the copy is provably wasteful.

## Proposed design (worked example)

Two genuinely different directions; the card asks us to be honest that **defer**
is one of them.

### Direction 1 — Smarter escape/liveness model (the "real fix")

Fire L0201 **only when the cloned value is dead after the call** — i.e. the
argument is never read again on any path. That is exactly `redundant_clone`'s
rule and it kills the false positives by construction: if the user uses `x`
after, the clone is *necessary*, so we stay silent; if `x` is never touched
again, the clone *is* wasteful and `move` is a real improvement worth surfacing.

```jet
fn demo() {
    let name = "Ada"

    // value reused after → clone is necessary → NO warning (today: warns, false positive)
    let a = User(name)
    let b = User(name)
    print(name)

    // value dead after → clone is wasteful → L0201 fires, suggests `move` (true positive)
    let c = User(name)
    // name never used again
}
```

This is the design that "cuts false-positive noise" honestly: it doesn't suppress
the lint, it makes the lint *correct*. Cost: a last-use / liveness analysis over
the function body, threaded through the four firing sites via one shared
`is_last_use(ident, after: span)` helper. That's real dataflow work in sema — not
huge, but not free, and it must be exact (a wrong "dead" claim that suggests
`move` on a still-live value would be an unsound suggestion that breaks the
user's program if followed).

**Discriminator check (does it hide real mistakes?):** No. The only case it
*newly* silences is "value reused after the call," where the clone is provably
needed — there is no move-vs-clone mistake to hide there, because `move` would be
*wrong*. It still fires on the wasteful case, which is the only case where `move`
is the right advice. So it silences false positives without hiding a real error.
That's the line that makes Direction 1 sound.

### Direction 2 — Principled suppression (cheap, honest, no dataflow)

If we don't fund liveness, suppress on the clearest false-positive class:
**downgrade L0201 to off-by-default**, surfaced only on explicit opt-in
(`jet run --lint=clones` or a `@audit`-style request), the way clippy handles its
noisy-but-true lints. The clone still happens (safe, correct); the user just
isn't nagged about a copy they usually want.

```shell
$ jet run app.jet                 # no clone warnings (quiet by default)
$ jet run app.jet --lint=clones   # opt-in: see every implicit clone, for tuning hot paths
```

This is *not* a real analysis improvement — it admits the lint is right but the
*default level* is wrong, exactly clippy's resolution. Cheap (one severity/level
flag, no dataflow), reversible, and honest.

### Direction 3 — Defer (do nothing until evidence)

Keep L0201 as-is until there's real evidence (a corpus of Jet programs, user
reports counting the noise) that quantifies the false-positive rate and confirms
which class dominates. I8 (simplicity ratchet) and the card both point here: we'd
be funding a dataflow pass (Direction 1) or a lint-level policy (Direction 2)
against a *hypothesized* noise level, before v1, with no measured baseline.

## Implementation sketch — file-level touchpoints

For **Direction 1** (if funded):

- New shared helper in sema (e.g. `Source/Sema/CheckerOwnership.rs` or a small
  `liveness` module): `is_last_use(name, after_span, fn_body) -> bool`, a
  last-use scan over the function body's statements after the call.
- Thread it through the four firing sites
  (`CheckerItems.rs:185`, `CheckerOwnership.rs:515`, `CheckerInfer.rs:2890`,
  `CheckerStdlib.rs:817`) so L0201 only pushes when `is_last_use` is true.
- `Source/Codegen/Expression.rs:1141/1304/1433` — unchanged; the clone still
  emits whenever `implicit_clone` is set (the *clone* is correct regardless; only
  the *warning* gate changes).

For **Direction 2** (cheap):

- A lint-level table / `--lint=` flag in `Source/main.rs` + the diagnostics
  driver; L0201 default = off. The four firing sites still set the
  `implicit_clone` flag (codegen unchanged); the *driver* decides whether to
  print.

For **Direction 3**: nothing. Add a tracking note + the evidence we'd want.

## Test plan

- **Direction 1:** the worked-example fixture above — assert L0201 fires *only*
  on the dead-after case, *not* on the reused-after cases. ui snapshots for both.
  Plus a regression fixture per firing site (4 sites → 4 cases) proving the shared
  helper gates all of them identically.
- **Soundness guard:** a fixture where `move` would be wrong (value used after)
  must *not* get a `move` suggestion — protects against an unsound "dead" claim.
- **Direction 2:** `jet run` produces no L0201; `jet run --lint=clones` produces
  the full set. Snapshot both.
- **Either:** existing L0201 ui snapshots (`tests/ui_lint/`) re-blessed to match
  the new firing policy; the *message text* is unchanged (no diagnostics.md edit).

## Risks & invariant check

- **I1/I2/I3** — the clone itself is unchanged and correct; codegen stays dumb.
  Only the *warning gate* moves. rustc never involved.
- **I4** — L0201 already has a code + snapshot; this changes *when* it fires, so
  snapshots re-bless, but no new diagnostic is created (text unchanged).
- **I8** — the tension lives here. Direction 1 *adds* analysis (a liveness pass);
  it's justified only if evidence shows the noise is real and Direction 2 is
  insufficient. Direction 2 is the ratchet-friendly middle. Direction 3 is the
  purest ratchet answer.
- **Soundness risk (Direction 1 only):** an incorrect last-use result that
  suggests `move` on a live value would produce advice that, if followed, breaks
  the program. The liveness analysis must be conservative (only claim "dead" when
  certain; on doubt, stay silent — silence is always safe since the clone is
  correct).

## Open decisions

1. **D-L0201** — smarter liveness gate (Direction 1) vs. off-by-default
   suppression (Direction 2) vs. defer-until-evidence (Direction 3). Card below;
   **defer is a real, recommendation-eligible option.**

## Proposed decision card(s)

### D-L0201 — How to cut implicit-clone (L0201) noise (rec C, defer)

L0201 warns on every implicit `.clone()` even when the user can't usefully avoid
it. Three honest responses:

- **Option A — Liveness gate (warn only on a wasteful clone).** Fire L0201 only
  when the value is dead after the call; stay silent when it's reused. Makes the
  lint *correct*, kills false positives by construction. Cost: a real last-use
  analysis threaded through four firing sites.

    ```jet
    let a = User(name)   // name reused below → silent (clone is necessary)
    print(name)

    let c = User(name)   // name never used again → L0201 (clone is wasteful, `move` helps)
    ```

- **Option B — Off-by-default + opt-in.** L0201 quiet by default; surfaced only on
  `jet run --lint=clones`. Cheap, no dataflow, clippy's resolution for
  true-but-noisy lints.

    ```shell
    $ jet run app.jet                 # quiet
    $ jet run app.jet --lint=clones   # opt-in, for tuning hot paths
    ```

- **Option C — Defer to post-v1, gather evidence (recommended).** Leave L0201 as
  it is. Before spending on A or B, collect a real corpus and count the
  false-positive rate, so the fix is sized to measured noise, not guessed noise.

    ```shell
    # no change today; the lint stays. Decision deferred until:
    #  - a corpus of real Jet programs exists, and
    #  - we can measure: of all L0201 fires, what fraction are "value reused after"
    #    (the false positives A would silence)?
    ```

**Recommendation: C (defer), with A as the eventual fix if evidence warrants.**
The card already says "revisit post-v1 with evidence," and I8 backs it: A is a
dataflow pass funded against a *hypothesized* noise level, before v1, with no
baseline — exactly the kind of speculative complexity the simplicity ratchet
exists to refuse. The honest sequence is: ship, measure the false-positive rate
on real programs, then choose. **If** the measurement confirms the noise is real,
**A** is the right fix (it makes the lint correct rather than merely quieter) and
**B** is the cheap stopgap if A's dataflow cost is too high for the milestone.
Picking A or B *now*, pre-evidence, is the move I8 is meant to stop.
