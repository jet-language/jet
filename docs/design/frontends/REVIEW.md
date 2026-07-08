# Frontend design — director challenge pass

Review + fixes across 7 surfaces × 3 families (Carbon/Paper/Pulse) + DESIGN-FAMILIES.md.
All fixes applied in place. Diagnostics re-pinned to real `tests/ui/*.stderr`. One-glow
discipline enforced on Pulse. No-theming leaks removed. Head/charset added to bare files.

Director's picks below are recommendations only — owner still chooses. Note: Carbon wins
most dense/instrument surfaces; a Carbon-leaning mix reads cohesive, with Paper as the
newcomer-facing swap on help/studio.

## canvas

Found → fixed:
- Diagnostic fabricated: E0204 quoted `net.tcp_reply(...)` (no `x`) under an `x`-borrow message → restored verbatim `both(&x, x)` @6:14 + caret from `borrow_conflict.stderr` (all 3).
- `#hud` id (aviation HUD) → `#viewbar` (all 3).
- Carbon palette empty-state didn't match paper/pulse invite → added "Try a shorter search."
- Pulse multi-glow: de-hotted brand "Canvas", breakpoint (→error red), paused-banner dot, replay slider, minimap active rect. Now the paused node + its exec pulse are the single hot cluster.
- NOTES "not a cockpit" → "not a dense instrument panel."
- Added doctype/lang/charset head (was bare `<title>`).

Residual: paper long-session-on-white fatigue (already flagged in NOTES); pulse counts the running node + wire pulse as one cluster (intended).

Director's pick: **Carbon** — closest to shipped Canvas.rs; density is right for a debugger/blueprint workbench; status band carries build/watch/diag/port at a glance.

## repl

Found → fixed:
- All 3 error blocks used ILLUSTRATIVE, wrong-format text (`error[E0308] this call wants an Int, got String`) → replaced with real `Error [E0112]` (arg type mismatch) from `arg_type_mismatch.stderr`: verbatim message + `Why:`/`Fix:`, family only styles the frame.
- NOTES box-drawing (all 3) regenerated with the real diagnostic; Carbon/Paper boxes re-padded to a clean uniform width.
- Pulse error: only the caret glows now (was error-label + string + caret all hot).

Residual: none material — these are doc-style stills, not live.

Director's pick: **Carbon** — persistent status band = whole-session state for a daily driver; Paper is the better teaching/newcomer swap.

## prompt

Found → fixed:
- Pulse cursor block stayed gradient in the "failing build" still, competing with the hot error segment → cursor de-hotted (cool). One glow per state now.
- Confirmed `E0308` is a real Jet code (bare-null-needs-type) — kept.

Residual: none.

Director's pick: **Pulse** — a 2-line prompt is exactly where "one glow that moves (cursor→spinner→error)" pays off; minimal and expressive.

## help

Found → fixed:
- Pulse over-applied the gradient (brand, cursor, breadcrumb, selected row, selected example all hot). De-hotted brand + cursor + breadcrumb to match Carbon/Paper; dropped example-glow on list screens so the selected row is the single glow.

Residual: fuzzy screen still shows selected row + its matched chars both warm — same element, acceptable.

Director's pick: **Paper** — help is the discovery/newcomer surface; the editorial reading view best serves the owner goal ("make exploration enjoyable").

## cli

Found → fixed: nothing required. Verified the build-failure block is jetpack runtime output (correctly NOT a compiler E-code, so no I4 snapshot owed). 5 flows + NO_COLOR present in all three; NO_COLOR fallbacks stated; Pulse discipline clean.

Residual: Pulse Flow 03 glows "43" three times — same value across scrollback, acceptable.

Director's pick: **Carbon** — status band + aligned resolve/build ledger fit a package/OS power CLI.

## devserver

Found → fixed:
- Diagnostic drift (all 3, terminal + overlay = 6 blocks): E0102 `Why` dropped `/ \`input\``; `Fix` fabricated a "did you mean `render`?" that E0102 never emits → restored verbatim from `comptime_if_unknown_fn_dropped.stderr` (`Fix: define it first (fn rende() { ... }), or call one that exists`).
- NOTES main verbatim block + 3 schematic boxes corrected to match.
- Pulse: de-hotted the chrome "live" pill/dot so the overlay's hot header bar is the single glow.

Residual: none material.

Director's pick: **Carbon** — status band flips `BUILD ● 1 error` for at-a-glance health while the last good build keeps serving; overlay diagnostic verbatim.

## studio

Found → fixed:
- Carbon signature class `.annun` (annunciator — cockpit term) → `.statband` (7 refs).
- Pulse broke its own one-glow rule: header wordmark, nav "3" count badge, and primary `.btn.hot` actions all glowed alongside each screen's subject. De-hotted wordmark + count badge → cool; the two `.btn.hot` CTAs → cool. Each screen's subject (gen tile / selected row / current-gen node / running step) is now the single glow, per NOTES.
- Added doctype/lang/charset head (was bare `<title>`).
- Verified all 8 screens functional + wired + source-backed in every family — no stubs, no dead/decorative controls, no "coming soon."

Residual: Carbon internal color tokens named `--sky/--advise/--caution/--alert` (annunciator-severity metaphor) — invisible names, left as-is (large churn, defensible as severity words) but flagged. Pulse `.btn.hot` CSS rule now unused (harmless).

Director's pick: **Carbon** — an admin/settings app rewards density; SET-vs-DEF provenance, line-select keys, and the sticky source pane are the strongest expression of "toggle = a line of source." Paper is the clean newcomer-facing alternative.

## DESIGN-FAMILIES.md

No change needed — token spec accurate; family signatures held after fixes.
