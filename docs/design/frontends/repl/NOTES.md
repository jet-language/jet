# Jet REPL — design options

Surface: `jet repl`. Three families, one option each. Feature set fixed
(banner, live highlight, completion, multiline, `name : Type = value` output
D-REPL16=B, diagnostics, `:quit :reset :load :type :help`). Diagnostic words +
codes snapshot-pinned (I4) — frame/color only is designed.

Shared transplants (all three): ptpython/fish **ghost autosuggest** from
history (→ accepts); **Tab completion menu** with signature column
(ptpython/nushell); Julia **`?name` inline docs** mode.

---

## A · Carbon — dense dark, status band

Signature: persistent **status band** under the title bar (SESSION/CORE/FUEL/
STEP with state LEDs). Step-indexed prompt `[n] ▸` replaces flat `user>`.
Value line = engineered readout, dotted rule name→value, type in select
magenta. Best for heavy daily users who want state always in view.

```
┌ jet repl ───────────────────────────────────── 80×24 ┐
│ SESSION ● live │ CORE ● loaded │ FUEL 65000 │ STEP 4 │
├──────────────────────────────────────────────────────┤
│ [4] ▸ grid.take(5)                                   │
│                                                      │
│   grid ····· : [String] = ["1","2","Fizz","4",..]    │
│                                                      │
│ [5] ▸ show("hi")                                     │
│                                                      │
│ Error [E0112]: `show` wants Int (a whole number)     │
│ for argument 1, but this is String (text)            │
│   --> <repl:6>                                       │
│     |                                                │
│   6 |     show("hi")                                 │
│     |          ^^^^                                  │
│  Why: every argument must match its parameter's type │
│  Fix: use Int (a whole number) here                  │
│ [6] ▸ _                                              │
└──────────────────────────────────────────────────────┘
```

NO_COLOR: LEDs drop, band stays as labelled text. ANSI-16: 1:1 map.

---

## B · Paper — light, foldable blocks

Signature: **fading hairline** leading label→value; each turn is a Warp-style
**block** you can fold/rerun. Light terminal — deliberate counter-position to
dark tools (ship `--dark` swap). Prompt is one accent chevron `›`. Errors are
flare-orange, never a red wall. Best for docs-adjacent, teaching, newcomers.

```
+-- jet repl -------------------------------- 80x24 --+
| Jet 0.9.2  interactive REPL   core loaded fuel 65k  |
| ?name for docs · :help commands · :quit exit        |
+-----------------------------------------------------+
|> grid.take(5)                                       |
|                                                     |
|  grid --------- : [String] = ["1","2","Fizz",..]    |
+-----------------------------------------------------+
|> show("hi")                                         |
|Error [E0112]: `show` wants Int (a whole number)     |
|for argument 1, but this is String (text)            |
|  --> <repl:6>                                       |
|    |                                                |
|  6 |     show("hi")                                 |
|    |          ^^^^                                  |
| Why: every argument must match its parameter's type |
| Fix: use Int (a whole number) here                  |
+-----------------------------------------------------+
```

NO_COLOR: blocks keep hairline rules, accents → bold weight.

---

## C · Pulse — dark, one glow

Signature: exactly **one hot→hot2 gradient glow per screen** — the live edge
(prompt chevron, selected completion row, just-computed value name, or the
error caret). All syntax is cool-toned so nothing competes. Prompt `[n] ▸`
with glowing chevron. Best when the REPL is a showpiece / demo surface.

```
### jet repl ############################### 80x24 ##
  JET  interactive REPL · v0.9.2
  core loaded · fuel 65k · :help · :quit

[4] ▸ grid.take(5)                     (▸ = glow)

  grid ····· : [String] = ["1","2","Fizz","4","Buzz"]
             (^ name grid glows: it is the live value)

[6] ▸ show("hi")
  Error [E0112]: `show` wants Int (a whole number)
  for argument 1, but this is String (text)
    --> <repl:6>
      |
    6 |     show("hi")
      |          ^^^^   (^ caret is the one glow)
   Why: every argument must match its parameter's type
   Fix: use Int (a whole number) here
```

Truecolor: gradient. 16-color: solid bright-red. NO_COLOR: glow → bold ▸.
