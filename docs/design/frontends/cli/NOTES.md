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

---

## hybrid.html — consequence-scaled output

**Core loop:** output scales with consequence — trivial ops print a quiet
ledger, long builds pin a live region that promotes finished lines into that
ledger, and mutations gate on a plan first; the scrollback is always the record.

One output model, three depths selected by what the command does, not by which
command it is. The ledger is the spine underneath all three.

| Source option | Transplanted aspect |
|---------------|--------------------|
| ledger | The spine: every finished line freezes into scrollback and never moves. This is tier 1 (trivial/read-only: `jetpack add`, `jet env`) in full. |
| live | Tier 2: a long build pins a bottom region that redraws in place, promotes each finished item up as a permanent ledger line, then collapses to a summary — the region *was* the record. |
| plan | Tier 3: any mutation (`jetos switch`, `jetpack gc`) prints a full `+/~/-` diff + closure/size delta, asks once `[y/N]`, then applies via the tier-2 region-into-ledger flow, each `✓` tied to a plan line. |

**Deliberately left out**
- plan's gate on trivial adds — an owner benchmark, but prompt fatigue on
  `jetpack add ripgrep` is a real cost; the consequence tier drops the gate for
  safe ops (`--yes` still available, gate still there for mutations).
- live's pinned region for short ops — a two-package add doesn't earn a progress
  bar; it just appends. The region only opens for genuinely long work.
- Two separate reporting mechanisms — apply after a plan is *not* a new format;
  it reuses the exact region-into-ledger flow, so there is one progress
  experience, not three (I8).

**Risks**
- The tier boundary ("trivial" vs "long" vs "mutating") must be a clear, stated
  rule, not a guess — misclassifying a mutation as trivial skips a needed gate.
- live region redraw math on resize; region must erase cleanly before any
  diagnostic prints (I4) or a bar mid-rewrite corrupts the error.
- non-TTY must never hang: region falls back to appended lines, and a mutation
  prints its plan then exits (needs `--yes` to apply).
- apply must stay transactional — a failed line abandons the plan, nothing
  changed.

```
tier 1 (trivial)          tier 2 (long build)         tier 3 (mutation)
  jetpack add ripgrep fd    jetpack build               jetos switch
    ✓ ripgrep 14.1.0          ✓ ripgrep 14.1.0          Plan  gen 42 → 43
    ✓ fd      9.0.0         ── live region ──────         + firefox  → 129.0
  2 packages ready ✓         building 31/42 · linux       ~ linux 6.9.7→6.10.1
                             ██████████░░░  compiling      - obsolete-lib → —
  jet env                       │ finished rows          Download 240 MB
    ✓ node     22.3.0            ▼ promote UP             Apply? [y/N] _
    ✓ postgres 16.3.0        jetpack  build ready ✓          │ y → region+ledger
  ── myproj ─ dev shell ──     (region → ledger line)        │   ✓ firefox (plan +)
                                                             │ N → nothing touched

failure (any tier): region erased → verbatim E0102 → stop; mutation abandons plan.
NO_COLOR/CI: no region, no gate — trivial appends; mutation prints plan + exits.
```
