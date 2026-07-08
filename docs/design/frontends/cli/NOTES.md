# CLI output — design options (jetpack / jetos / jet env)

Three families, same flows: `jetpack add`, `jet env` enter/exit, `jetos switch`,
`jetpack gc`, build-failure w/ `--shell-on-fail`, NO_COLOR degrade. Std-only ANSI
(I6): SGR, box-drawing, Braille spinner, `\r` redraw — no TUI crate. Diagnostic
words snapshot-pinned (I4); frame/color free, words fixed. "hangar" = shipped
store name (kept). No theming.

Baseline kept from `Jetpack/Output.rs`: `jetpack` gutter, `▸` detail, `✓ name ver
state` ledger row, threshold `rule()`, Braille spinner, `error()/fix:` block,
`Resolved N packages in Xms` (uv). All three families reuse these; they differ in
color/emphasis only.

---

## Carbon — dense dark, Linear/Zed-class

- **Signature:** persistent status band — labeled state lights above the scroll
  (`HANGAR ● OK  BUILD ● OK  CLOSURE ● +34M`). Whole-system state in one glance,
  legible even after the log scrolls past.
- **Rationale:** for people in the tool all day. Max density, strict mono
  alignment, tabular numerals. Cyan accent / magenta select / amber warn.
- **Transplants:** uv resolve summary; nh generation diff (`+ − ↑ ↓` + closure
  delta), four colors so diff scans without reading versions.

```
$ jetos switch
 HANGAR ● OK   BUILD ● OK   CLOSURE ● +34M   SWITCH ● pending
  jetos  building generation
         ▸ realised 1240 / 1240 derivations
  jetos  generation 42 → 43

  changes
    + hello           2.12.1
    ↑ ripgrep         14.1.0 → 14.1.1
    ↓ nodejs          22.5.0 → 20.15.1
    − obsolete-tool   1.0.0
    4 changed · +1 −1 ↑1 ↓1 · closure 2.1 GiB (+34 MiB)

  jetos  switch to generation 43? [Y/n] y
  jetos  activated generation 43 ✓
```

---

## Paper — light editorial, Stripe-docs class

- **Signature:** fading hairline — rule starts solid, thins to nothing, leads
  label → value / step → step. Truecolor fade on TTY; degrades to dash density
  `───╌╌`.
- **Rationale:** counter-position to every dark tool. Light ground, generous
  whitespace, type carries hierarchy. Sparse warm accent (flare orange) used
  once per screen.
- **Transplants:** uv resolve summary reads like a printed receipt; nh diff with
  additions/upgrades cool+green, single downgrade + size delta in flare.

```
$ jetpack gc
  jetpack  hangar disk report

    generations  ──────╌╌ ·  12 kept · 8 reclaimable
    store paths  ──────╌╌ ·  1,204 live · 318 dead
    reclaimable  ──────╌╌ ·  4.7 GiB

  jetpack  remove 318 dead paths + 8 generations? [y/N] y
  jetpack  freed 4.7 GiB ✓
```

---

## Pulse — dark bold, disciplined

- **Signature:** single gradient glow — exactly one hot→hot2 highlight per
  screen on the live thing (building pkg, target generation, reclaimable figure).
  Everything else cold cool/gray. Absence of glow is also signal (env exit).
- **Rationale:** energetic but never noisy. Heat lives in one place; the eye goes
  straight to what's live. Truecolor gradient where supported, 16-color hot
  fallback.
- **Transplants:** uv resolve summary; nh diff rendered cold, only target
  generation glows through confirm → activate.

```
$ jetpack add nixpkgs:ripgrep
  jetpack  resolving 1 request
  jetpack  Resolved 4 packages in 312ms
           ⠹ building ripgrep 14.1.1 …      ← glow (hot→hot2)
           ✓ pcre2      10.44    cached
           ✓ oniguruma  6.9.9    cached
  jetpack  added ripgrep to env.jet ✓
```

---

## NO_COLOR / non-TTY (all three converge)

Off-TTY every family degrades to the same quiet aligned baseline: no SGR,
spinner inert (Braille never prints), `✓ → [ok]`, rules → ASCII `--`. Alignment
carries all meaning color did.

```
$ jetpack add nixpkgs:ripgrep
  jetpack  resolving 1 request
  jetpack  Resolved 4 packages in 312ms
           [ok] ripgrep    14.1.1   built 8s
           [ok] pcre2      10.44    cached
           [ok] oniguruma  6.9.9    cached
  jetpack  added ripgrep to env.jet
  -- myproj - temporary shell - exit to leave --------------
```
