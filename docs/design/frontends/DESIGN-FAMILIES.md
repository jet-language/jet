# Jet frontend design families

Three coherent design languages, each applied across every user-facing
surface (Canvas, jetos Studio, REPL, dev server, jetpack/jet env CLI, jet
prompt, `jet ?` help). The owner picks per surface; because every family
shares the same geometry, spacing scale, and voice rules, any mix still
reads as one product. Mockups live in sibling directories, one per surface.

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

## Family A — Glass Cockpit

Modern avionics suite (A350/G3000 EFIS). Data as instruments: bezeled
tiles, engineered grid, annunciator strips. Calm, dense, precise.

- `panel   #0B1119` — night flight-deck background
- `bezel   #16202E` — raised panel / tile borders
- `sky     #3FC6FF` — primary accent, active, links (EFIS cyan)
- `advise  #D678FF` — selected/guidance values (FMS magenta)
- `caution #FFB454` — warnings (amber annunciator)
- `alert   #FF5C5C` / `ok #58D68D` — failure / green light
- Text `#D9E6F2`; secondary `#7E93A8`.
- Type: system sans for chrome, ui-monospace for every value/readout;
  data readouts letter-spaced, tabular numerals.
- ANSI mapping: cyan=accent, magenta=selected, yellow=caution, green=ok,
  red=alert, bright-black=secondary.
- Signature: the annunciator strip — a persistent one-line status band of
  labeled lights (BUILD ● OK  WATCH ● 3 files  PORT 8080).

## Family B — Slipstream

High-altitude daylight. Light-first, editorial, airy; hairline contrail
rules and a horizon gradient. The counter-position to every dark dev tool.

- `paper    #F7F9FC` — cool white ground (not cream)
- `ink      #1B2733` — primary text
- `strato   #1E63E9` — primary accent, actions, links
- `horizon  #DCEBFF → #FFFFFF` — header gradient (sky meets page)
- `contrail #C4D2E0` — hairline rules, borders
- `flare    #FF7A45` — sparse secondary accent (flight-suit orange), warnings
- Type: system sans display at generous sizes and tight leading; body
  comfortable; mono only where content is code.
- ANSI mapping (dark terminals): blue=accent, white/bold=headings,
  bright-black=rules, yellow→flare for warnings, green=ok, red=error.
- Signature: the contrail rule — a thin line that begins solid and fades,
  used to lead the eye from a label to its value or from step to step.

## Family C — Afterburner

Night sortie with the burner lit. Expressive, high-energy, terminal-native
showmanship (Charm-school), but disciplined: heat lives in one place per
screen.

- `tarmac #120E1A` — deep violet-black ground
- `ember  #FF6B35` — primary accent
- `burn   #FF3D81` — gradient partner of ember (ember→burn for glow/motion)
- `ion    #8B7CFF` — secondary accent, structure, links
- `vapor  #E8E3F2` — primary text; secondary `#8E86A3`
- Type: bold condensed-feel display (system sans, heavy weights, tight
  tracking) for titles; mono for code; oversized single-glyph icons.
- ANSI mapping: red/bright-red=ember, magenta=burn, blue/bright-blue=ion,
  bright-black=secondary; truecolor gradient where supported, 16-color fallback.
- Signature: the burn gradient — exactly one ember→burn glow per screen
  (active tab, running task, current selection); everything else stays cold.
