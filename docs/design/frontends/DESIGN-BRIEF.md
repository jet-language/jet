# Jet frontend design brief (v2)

Rules for every mockup and ballot in this directory. v1 (palette-family
options) was rejected: the options differed only in paint. **UX first, UI
second.** Each ballot option is a distinct UX archetype — different
information architecture, interaction model, and primary workflow. Test:
state each option's core loop in one sentence; if two sentences match,
it's the same option twice — start over.

## One visual system, not per-option themes

- **TUI theme is fixed and shared** across REPL, prompt, help, CLI, and
  dev-server terminal output. It is not on any ballot. Palette (ANSI-16
  safe, truecolor upgrade): cyan = accent/active, green = ok, yellow =
  warning, red = error, magenta = selection/emphasis, bright-black =
  secondary. Bold sparingly, dim for scaffolding. NO_COLOR and non-TTY
  degrade to aligned plain text with identical wording.
- **GUI surfaces** (Canvas, Studio) share one product visual language:
  near-black ground `#0B1119`, raised panels `#16202E`, accent `#3FC6FF`,
  ok/warn/error `#58D68D`/`#FFB454`/`#FF5C5C`, text `#D9E6F2`, secondary
  `#7E93A8`; system font stack, ui-monospace for values, 4/8/12/16/24/40
  spacing, 6px panel radius. A light theme may ship later as a
  preference; it is not an option axis.
- Options within a surface may differ visually ONLY where the archetype
  demands it (a notebook REPL has blocks; a line REPL doesn't).

## Copy rules

- Plain functional words. **No invented jargon, no branding in UI copy**
  ("fuel", "core", codenames — banned). "Jet"/"jetpack"/"jetos" appear
  only as product names. No metaphors or motifs (see v1 rejection).
- Professional tool, joyful to use: delight comes from responsiveness,
  smart defaults, and moments of polish (a satisfying completion, a
  perfect error) — never from decoration or mascot energy.
- Voice: active, sentence case. Errors say what happened and the fix.
  Empty states are invitations to act. Buttons name their action.
- Diagnostics text is snapshot-pinned (I4): frames/color free, words and
  codes verbatim from tests/ui.

## Hard constraints

- TUIs: std-only ANSI (I6) — SGR, box-drawing, Braille spinners,
  alt-screen; no TUI crate.
- GUIs: self-contained HTML/CSS/JS served by the jet binary; no CDN, no
  external assets.
- Keyboard-first, visible focus, reduced-motion respected.

## The option axis per surface (UX archetypes)

Mockups: one HTML file per archetype, named for the archetype (e.g.
`workbench.html`, `notebook.html`) — never for a color scheme.

- **Canvas**: e.g. docked IDE workbench · command-palette-first infinite
  canvas with floating chrome · synced source⇄graph dual-lens editor ·
  type-directed guided authoring. Each must state its core loop.
- **Studio**: e.g. grouped settings app · changeset-review-first
  (everything staged, diffed, then applied) · ops-dashboard-first (fleet/
  services live board, config on drill-in) · projectional source editor
  with inline GUI controls.
- **REPL**: e.g. enhanced classic line REPL · block/notebook session
  (fold, rerun, pin outputs) · pane workspace (session + bindings
  inspector + docs).
- **Prompt**: e.g. minimal single-line · rich two-line segments ·
  adaptive (silent when clean, expands on events).
- **Help (`jet ?`)**: e.g. instant command-palette overlay · two-pane
  browser app · task-oriented guided explorer with runnable recipes.
- **CLI output**: e.g. quiet scrolling ledger · pinned live status region
  over a scrolling log · plan→confirm→apply phased output.
- **Dev server**: e.g. minimal terminal + rich browser overlay · terminal
  status dashboard · browser-first control strip with near-silent
  terminal.

These lists are starting points — replace any archetype with a stronger
one, keep 3–4 genuinely distinct options per surface.
