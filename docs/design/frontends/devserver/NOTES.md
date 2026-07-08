# Dev server (`jet dev --target=web`) — archetype notes

3 UX archetypes, ONE shared TUI theme + ONE shared GUI product language
(DESIGN-BRIEF: ground `#0B1119`, panel `#16202E`, accent `#3FC6FF`, ok/warn/
error `#58D68D`/`#FFB454`/`#FF5C5C`). Only the *split of richness* between
terminal and browser changes.

Truth from `Source/CmdDevWeb.rs`: compile-once → serve `build/` → watch mtime
→ rebuild on save. Browser reload = **poll `/__jet_dev_version` every 400ms**
(no WebSocket, no SSE — I6). A broken save keeps serving the last good build.
Startup already prints `serving http://localhost:PORT — watching FILE …` and
`Canvas: …/canvas`.

Diagnostic (E0102) verbatim from `tests/ui/comptime_if_unknown_fn_dropped.stderr`.

Std-only ANSI (I6): `\r` line rewrite, cursor-home / cursor-up for pinned
header, SGR color. Browser overlay = self-contained HTML/CSS injected next to
the existing live-reload script (`inject_live_reload`).

Core-loop test — one sentence each, all distinct:

| # | file | core loop |
|---|------|-----------|
| 1 | quiet.html | Live in the browser; the terminal is a three-line daemon. |
| 2 | dashboard.html | The terminal is the primary view (pinned status header + request log); the browser stays clean. |
| 3 | paired.html | Glance at terminal or browser corner — same status, same error. |

quiet and dashboard are deliberate opposites (rich-browser/silent-terminal vs
rich-terminal/silent-browser); paired refuses the split and mirrors both.

---

## 1 — Quiet terminal, rich browser

**Loop:** live in the browser; terminal is 3 lines (URL, watch target, last
rebuild) then silent. Full error overlay, reload states, rebuild toast all in
the page.

**Rationale:** matches how web devs actually work — eyes on the rendered page.
Best-in-class error surfacing (vite overlay). Terminal stays a quiet daemon.
Cost: needs a real in-browser overlay; headless/CI has no browser to host it.

**Transplants:** vite/next error overlay (backdrop → window → message → file →
frame + Esc dismiss); vite rebuild toast; dim-last-good-page while building.

**Risks:** browser is the only rich surface — a build error with no tab open
is invisible until you look (falls back to terminal diagnostic); overlay CSS
must stay self-contained (no CDN).

```
 terminal (steady state)          browser overlay (on error)
 ┌───────────────────────┐   ┌──────────────────────────────────┐
 │ jet dev  serving      │   │ localhost:8080                   │
 │   :8080               │   │ ┌ Build failed ───────── app.jet ┐│
 │   watching app.jet    │   │ │ Error [E0102]: nothing named   ││
 │ ✓ rebuilt 0.4s 14:22  │   │ │   `nonexistent_function_xyz` …  ││
 └───────────────────────┘   │ │  8 | nonexistent_function_xyz() ││
                             │ │    | ^^^^^^^^^^^^^^^^^^^^^^^^     ││
  toast: ● rebuilt in 0.4s   │ │ Why: only functions defined …   ││
                             │ └────────────────────────────────┘│
                             └──────────────────────────────────┘
```

---

## 2 — Terminal dashboard

**Loop:** terminal is the primary view — pinned header (status dot, port,
clients, last build) over a scrolling request/rebuild log with timings. Browser
shows only a thin error banner pointing back.

**Rationale:** best for terminal-centric devs and multi-client debugging (see
every request + timing). No duplicated detail across surfaces. Cost: header
needs cursor-home redraw; browser is intentionally information-poor.

**Transplants:** next/astro dev request log; a status header line (nom/uv
header idea); client count derived free from the 400ms pollers.

**Risks:** pinned header needs alt-screen/cursor math and degrades on non-TTY
(becomes a plain status line); browser banner must not tempt scope creep into a
second overlay.

```
 ┌ jet dev ────────────────────────────────────────────┐
 │ ● ready   localhost:8080   2 clients   last build 0.4s│
 └──────────────────────────────────────────────────────┘
  14:22:03  GET  /            200  2ms
  14:22:07  save app.jet  →  rebuilt 0.4s
  14:23:01  save app.jet  →  error E0102
  Error [E0102]: nothing named `nonexistent_function_xyz` …
    8 | nonexistent_function_xyz()
      | ^^^^^^^^^^^^^^^^^^^^^^^^

 browser: ● Build failed · E0102 — details in the terminal
```

---

## 3 — Status parity

**Loop:** terminal one-line status and browser corner strip mirror each other
exactly (building / ok / error / n clients); on error both expand the identical
verbatim diagnostic frame. Glance at either side, same truth.

**Rationale:** no "which surface is authoritative?" question — they are the
same by construction (both read the same version poll). Lowest cognitive
overhead. Cost: neither surface is a deep dashboard; detail is the diagnostic
frame only.

**Transplants:** shared status vocabulary; one diagnostic frame rendered twice
(ANSI + CSS) from the same snapshot text.

**Risks:** must genuinely keep both in lockstep (single source = the poll) or
parity is a lie; the frame is the only detail level — no request log, no deep
metrics.

```
 terminal                          browser corner strip
 jet dev  ● error · E0102 · 2 cl.  ┌ localhost:8080 ─────────────┐
 ┌ E0102 ─────────────────────┐    │  (app, dimmed)              │
 │ Error [E0102]: nothing      │    │  ┌ E0102 ─────────────────┐ │
 │   named `nonexistent_...`   │    │  │ Error [E0102]: nothing │ │
 │  8 | nonexistent_function() │    │  │  8 | nonexistent_fn()   │ │
 │    | ^^^^^^^^^^^^^^^^^^^^^^  │    │  │    | ^^^^^^^^^^^^^^^^^^  │ │
 │ Why / Fix …                 │    │  └────────────────────────┘ │
 └─────────────────────────────┘    │  ● error · E0102 · 2 clients │
   (same frame, same words)         └─────────────────────────────┘
```
