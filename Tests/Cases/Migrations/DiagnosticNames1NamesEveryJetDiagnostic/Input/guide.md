# Reading a report

```
Error [E0113]: This value is String (text), but `f` returns Int
 More: jet-lang.dev/e/E0113
Run `jet explain E0113` to learn more
```

A lint heading such as `Lint [jet.UnusedImport]: …` names its lint, and
`#Allow(jet.UnusedImport, "kept for the demo")` keeps it quiet [jet.UnusedImport].
A library's own `Error [game.FrameHold]: …` keeps its name.
A runtime stop reads `Stop [E3001]: …` and links jet-lang.dev/e/E3001.
The report object `{"code":"E0113","what":"x"}` gains its name.
`jet.NotARegisteredDiagnostic` names no row and stays.
