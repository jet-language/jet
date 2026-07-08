# Jet frontend design families

Three coherent design languages, each applied across every user-facing
surface (Canvas, jetos Studio, REPL, dev server, jetpack/jet env CLI, jet
prompt, `jet ?` help). The owner picks per surface; because every family
shares the same geometry, spacing scale, and voice rules, any mix still
reads as one product. Mockups live in sibling directories, one per surface.

**No theming.** No metaphors, mascots, or decorative motifs of any kind —
no aviation, space, or "jet" imagery, and no themed vocabulary in UI copy.
Every visual element earns its place functionally. "Jet" appears only as
the product name. The families differ in tone and density, not in theme.

Shared, family-independent rules:

- Spacing scale: 4 / 8 / 12 / 16 / 24 / 40 px. Radius: 6px panels, 3px chips.
- Voice: active, plain, sentence case. Errors state what happened and the fix,
  never apologize. Empty states are invitations to act.
- Diagnostics text is product copy pinned by snapshots (I4): frames, color,
  and layout may change; the words and codes may not.
- TUIs are std-only ANSI (I6): every design must be renderable with SGR
  escape codes, box-drawing, and Braille spinners — no TUI crate assumed.
- GUIs are self-contained HTML/CSS/JS served by the jet binary: no CDN
  fonts, no external assets. System font stacks only.
- Keyboard-first everywhere; visible focus; NO_COLOR degrades to layout-only.

## Family A — Carbon

Dense, dark, engineered. The professional instrument for people who live
in the tool eight hours a day (Linear/Zed-class). Information density with
strict alignment; every value monospaced and scannable.

- `ground  #0B1119` — background
- `raised  #16202E` — panels, tile borders
- `accent  #3FC6FF` — active, links, primary actions
- `select  #D678FF` — current selection / focused value
- `warn    #FFB454` · `error #FF5C5C` · `ok #58D68D`
- Text `#D9E6F2`; secondary `#7E93A8`.
- Type: system sans for chrome, ui-monospace for every value/readout;
  tabular numerals, letter-spaced labels.
- ANSI mapping: cyan=accent, magenta=selected, yellow=warn, green=ok,
  red=error, bright-black=secondary.
- Signature: a persistent one-line status band of labeled state lights
  (BUILD ● OK  WATCH ● 3 files  PORT 8080) — whole-system state in one glance.

## Family B — Paper

Light-first, calm, editorial (Stripe-docs class). The counter-position to
every dark dev tool: generous whitespace, hairline structure, type doing
the hierarchy work.

- `paper    #F7F9FC` — cool white ground (not cream)
- `ink      #1B2733` — primary text
- `accent   #1E63E9` — actions, links
- `wash     #DCEBFF → #FFFFFF` — soft header gradient
- `hairline #C4D2E0` — rules, borders
- `flare    #FF7A45` — sparse secondary accent, warnings
- Type: system sans display at generous sizes, tight leading; mono only
  where content is code.
- ANSI mapping (dark terminals): blue=accent, white/bold=headings,
  bright-black=rules, yellow=warn, green=ok, red=error.
- Signature: the fading hairline — a thin rule that starts solid and fades,
  leading the eye from a label to its value or from step to step.

## Family C — Pulse

Dark, bold, energetic (Charm-school terminal-native polish) — but
disciplined: exactly one hot highlight per screen; everything else cold
and quiet.

- `ground #120E1A` — deep violet-black background
- `hot    #FF6B35` — primary accent
- `hot2   #FF3D81` — gradient partner (hot→hot2 marks the active thing)
- `cool   #8B7CFF` — secondary accent, structure, links
- `text   #E8E3F2`; secondary `#8E86A3`
- Type: heavy-weight system sans, tight tracking for titles; mono for code.
- ANSI mapping: red/bright-red=hot, magenta=hot2, blue/bright-blue=cool,
  bright-black=secondary; truecolor gradient where supported, 16-color
  fallback.
- Signature: the single gradient glow — one hot→hot2 highlight per screen
  (running task, active tab, current selection); its absence is also signal.
