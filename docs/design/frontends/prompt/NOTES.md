# Jet prompt — design options

Surface: the shell prompt injected into every `jet env` dev shell
(starship-class). Shows env name, Jet version, build/test state, git branch,
dirty markers. Hard height budget: **≤2 lines** (info + input); transient
collapse of finished commands keeps scrollback to 1 line each.

Shared transplants: **starship/oh-my-posh transient prompt** (finished
commands collapse to verb + result + duration); **starship segment model**
(env / version / vcs); **Braille spinner** ⣾⣽⣻⢿⡿⣟⣯⣷ for running work.

States shown per option: normal · dirty-git · failing-build · long-running.

---

## A · Carbon — status-band prompt

Signature: the status band as a prompt. Info line = flat space-separated
segments with condition **LEDs** (green ok / red fail). Failing build shows
the error code inline. Best for engineers who want full state every prompt.

```
webapp · jet 0.9.2 · ● tests ok · ⎇ main          (normal)
▸ _

webapp · jet 0.9.2 · ● tests ok · ⎇ main ●3 +42 −7 (dirty)
▸ git add .

webapp · jet 0.9.2 · ● build failed E0308 · ⎇ main ●1 (fail)
▸ _

▸ jet build            ● 1.2s                      (transient)
▸ jet test             ● 0.8s
webapp · ⣾ running jet bench · 12s · ⎇ main
```

NO_COLOR: LEDs → [ok]/[FAIL] tags.

---

## B · Paper — quiet typographic prompt

Signature: fading hairline between segments, trailing into the chevron. A
clean passing shell **hides the info line entirely** — absence is the
all-clear. Only a failed build raises its voice, phrased as a next action.
Light terminal (`--dark` inverts). Best for calm, low-noise shells.

```
webapp ─ jet 0.9.2 ─ main                          (normal)
> _

webapp ─ jet 0.9.2 ─ main  ✎ 3 · +42 −7            (dirty)
> git add .

webapp ─ jet 0.9.2 ─ main  ✎ 1                     (fail)
build failed · E0308 in label() · run jet build to see it
> _

> jet build   ✓ 1.2s                               (transient)
> jet test    ✓ 0.8s
webapp ─ ⣾ jet bench 12s   Ctrl-C to stop
```

NO_COLOR: hairlines stay, markers → [!] tags, ✓/✎ keep as glyphs.

---

## C · Pulse — one moving glow

Signature: exactly one hot→hot2 glow, and it **moves to whatever matters**:
the cursor when idle, the failed segment when broken, the spinner when busy.
A dirty tree stays cool (normal working state). Best when the terminal is a
showpiece. Absence of glow anywhere = nothing needs you.

```
webapp · jet 0.9.2 · ✓ tests · ⎇ main              (normal)
▸ _                        (▸ glows)

webapp · jet 0.9.2 · ✓ tests · ⎇ main ●3 +42 −7    (dirty)
▸ git add .                (▸ glows; markers cool)

webapp · jet 0.9.2 · ✗ build E0308 · ⎇ main        (fail)
▸ _                        (✗ segment glows; ▸ cool)

▸ jet build   ✓ 1.2s                               (transient)
webapp · ⣾ jet bench 12s · ⎇ main   (spinner glows)
```

Truecolor: gradient. 16-color: solid bright-red. NO_COLOR: glow → bold
leading ▸/✗/⣾.
