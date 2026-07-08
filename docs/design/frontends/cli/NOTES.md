# CLI output — archetype notes

3 UX archetypes, ONE shared TUI theme (DESIGN-BRIEF palette: cyan active,
green ok, yellow warn, red error, magenta emphasis, bright-black secondary;
ground `#0B1119`, text `#D9E6F2`). Flows identical across options — `jetpack
add`, `jet env`, `jetos switch`, `jetpack gc`, build failure. Only how
progress + results are *experienced* changes.

Diagnostic (E0102) is verbatim from `tests/ui/comptime_if_unknown_fn_dropped.stderr`,
color-free (I4), identical in all three.

Std-only ANSI (I6): SGR color, `\r` line rewrite, `CSI nA` cursor-up region
rewrite, `CSI 2K` erase line. No TUI crate.

Core-loop test — one sentence each, all distinct:

| # | file | core loop |
|---|------|-----------|
| 1 | ledger.html | Read top to bottom once; scrollback is the permanent record. |
| 2 | live.html | Watch one bottom region; finished work scrolls up clean above it. |
| 3 | plan.html | Decide from a full plan, answer once, then it applies almost silently. |

---

## 1 — Ledger

**Loop:** read top→bottom once; active line spins, done lines freeze,
scrollback is the record.

**Rationale:** refines what jetpack already does (`Output.rs` gutter + row +
spinner). Zero learning cost, perfect scrollback/log fidelity, dead simple to
implement. Cost: a very long op is a wall of lines; no single "where am I now".

**Transplants:** cargo `Compiling…/Finished`; jetpack `status`/`ok`/`row`;
Braille spinner already in `Output.rs::spinner`.

**Risks:** big multi-package builds get noisy; no aggregate progress; the one
spinner line is the only "live" signal.

```
  jetpack  add ripgrep fd
           ▸ resolving from nixpkgs
           ✓ ripgrep  14.1.0  built 6s
           ✓ fd       9.0.0   cached
  jetpack  2 packages ready  ✓

  jetpack  env
           ✓ node     22.3.0  cached
           ✓ postgres 16.3.0  cached

  ── myproj ─ dev shell · exit to leave ──────────────

  build fails ▸ diagnostic prints inline, verbatim, no spinner left hanging
```

---

## 2 — Live region

**Loop:** watch one pinned bottom region (bar + counts + elapsed + size);
finished items promote up into clean scrollback; region collapses to a summary.

**Rationale:** best "where am I now" for long ops — one aggregate `31/42`
beats 42 log lines. Scrollback stays minimal. Cost: more ANSI (cursor-up
region), and it must clear cleanly before any error or diagnostic.

**Transplants:** uv/bun/pnpm pinned progress area; pnpm `Progress: resolved/
reused/added` counters; nom-style build counter.

**Risks:** region redraw math on resize; non-TTY loses the region (degrades to
appended lines); must erase region before printing a diagnostic.

```
  jetpack  add ripgrep fd
           ✓ ripgrep  14.1.0  built 6s
  ── live region · redraws in place ──────────────────
  realizing 1/2 · fd            00:06 · 42 MB
  ████████████████░░░░░░░░  unpacking
  ────────────────────────────────────────────────────
        │ finished items promote UP, region stays pinned
        ▼
  jetpack  2 packages ready in 6s  ✓   (region replaced)
```

---

## 3 — Plan

**Loop:** every mutation prints a full plan (add/change/remove + download +
closure/generation delta), asks once `[y/N]`, then applies terse, each done
line tied to a plan line.

**Rationale:** matches the owner's nh/terraform benchmark; safest for
destructive ops (`gc`, `jetos switch`); decide from the diff, apply blind.
Cost: an extra step + prompt on every mutation — heavier for trivial adds.

**Transplants:** terraform `+/~/-` ledger + `Plan: N to add…` + `Enter a
value`; nh generation diff + closure delta + boot target.

**Risks:** prompt fatigue on small ops (needs a sensible `--yes` / auto-apply
for trivial/CI); non-TTY must not hang — prints plan, exits without applying.

```
  jetos  switch

  Plan  generation 42 → 43 · 1 add, 1 change, 1 remove

    + firefox       —      → 129.0
    ~ linux         6.9.7  → 6.10.1
    - obsolete-lib  1.2.0  → —

  Download 240 MB · closure +18 MB · rebuild boot + current

  Apply? [y/N] ▏
     │ y → quiet apply, one ✓ per planned line
     │ Enter/N → nothing touched
```
