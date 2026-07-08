# Dev server — design options (`jet dev --target=web`)

Two surfaces per family: terminal output (start banner, watch status, rebuild
lines, compile-error display) + in-browser error overlay (shown over the user's
app on compile failure) with reconnect/reload states. Live-reload is polling
(`/__jet_dev_version` @ 400ms) — no websocket (I6). Overlay is self-contained
injected HTML/CSS/JS, no CDN. Compile-error text inside the overlay is the
**verbatim Jet diagnostic** (I4) — designed chrome only frames it, never rewords.
Start-banner words are free (not a diagnostic). No theming.

Shared truths: front-end compile errors never write `build/` — last good build
keeps serving, overlay auto-clears on next clean polled version. Three reconnect
states: Rebuilding (save) · Reloading (version bump) · offline–retrying (poll
failed).

Verbatim diagnostic embedded in all three overlays:

```
Error [E0102]: nothing named `rende` exists here
  --> app.jet:14:5
    |
 14 |     rende(scene)
    |     ^^^^^
 Why: only functions that have been defined (or built in, like `print` / `input`) can be called
 Fix: define it first (fn rende() { ... }), or call one that exists
```

---

## Carbon — status band + full-bleed dark overlay

- **Signature:** status band above the log flips `BUILD ● 1 error` — the app's
  health is legible at a glance regardless of scroll.
- **Overlay:** vite-class full-bleed dark card, header with clickable
  `file:line`, dismiss tip. Diagnostic body verbatim.
- **Transplant:** vite error overlay (full viewport, file-link, code-frame, tip).

```
 SERVE ● :8080   WATCH ● app.jet   BUILD ● 1 error   CLIENTS ● 1
  jet dev  web target
    serve   http://localhost:8080
    watch   app.jet
    ready in 640ms · Ctrl-C to stop
  14:22:08  app.jet changed → rebuilt in 210ms ✓
  14:22:19  app.jet changed → 1 error
  ┌ Build failed ───────────────────── app.jet:14:5 ┐
  │ Error [E0102]: nothing named `rende` exists here │
  │   --> app.jet:14:5   14 | rende(scene)  ^^^^^     │
  │  Why: … can be called   Fix: define it first, … │
  └ Fix the code to dismiss · Esc to hide ───────────┘
```

---

## Paper — fading hairline + light overlay (the bet)

- **Signature:** fading hairline leads timestamp → result on each rebuild line.
- **Overlay:** the risk — a **light** error overlay. A compile error as a calm
  readable card, not a dark takeover. Fading hairline under the header; blue
  file-link; diagnostic body verbatim.
- **Transplant:** vite overlay structure, re-lit for a light ground.

```
  jet dev  web target
    serve   http://localhost:8080
    ready in 640ms · Ctrl-C to stop
  14:22:08  app.jet changed ──╌ · rebuilt in 210ms ✓
  14:22:19  app.jet changed ──╌ · 1 error
  ┌ Build failed   app.jet:14:5 ─────────────────╌╌  ┐  (light card)
  │ Error [E0102]: nothing named `rende` exists here │
  │   --> app.jet:14:5                                │
  │ 14 |  rende(scene)                                │
  │       ^^^^^                                        │
  │  Why: … can be called                             │
  │  Fix: define it first, or call one that exists    │
  └ Fix the code to dismiss · Esc to hide ────────────┘
```

---

## Pulse — one gradient glow + dark overlay

- **Signature:** the live server's `jet dev` banner glows hot→hot2; on failure
  the heat moves to one gradient bar across the overlay header. One warm thing
  at a time.
- **Overlay:** dark card, single hot→hot2 bar over the failure header, cool
  file-link, diagnostic body verbatim.
- **Transplant:** vite overlay; heat concentrated per the family rule.

```
  jet dev  web target            (banner glows = server live)
    serve   http://localhost:8080
    ready in 640ms · Ctrl-C to stop
  14:22:08  app.jet changed → rebuilt in 210ms ✓
  14:22:19  app.jet changed → 1 error          (heat → red)
  ▟▟▟▟▟▟▟▟▟▟ hot→hot2 bar ▟▟▟▟▟▟▟▟▟▟▟▟▟▟▟▟▟▟▟▟
  │ Build failed                    app.jet:14:5 │
  │ Error [E0102]: nothing named `rende` exists… │
  │   --> app.jet:14:5   14 | rende(scene) ^^^^^ │
  │  Why: … can be called  Fix: define it first, … │
  └ Fix the code to dismiss · Esc to hide ───────┘
```

---

## Reconnect / reload states (all three)

Polling model (I6): small corner pill, never a takeover.

```
● Rebuilding…            (save detected)
● Reloading…             (new version 5 → location.reload())
● Dev server offline — retrying   (poll failed; clears when it answers)
```
