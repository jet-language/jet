# #3165: enum text parsing keeps case, wire and display names distinct

Closer11, 2026-09-29. Binary: `~/.cache/jet-dev/safe-jet.sh` → `jet-debug-snapshot14`.

## Question

How does a Jet program turn text (for example a CLI choice) into a payload-free
enum today? Do the source case name, the grouped name, the Codable wire rename
and the display text stay distinct? Is there a generated parser, and if not,
what would a Core API ballot look like?

## Reconciled prior receipts (criterion 4)

- The retired receipt the finding names is #566 / c03qvxt7 ("Explicit conversion
  direction", done, D-SHAPE-CONVERT1=A). Its four criteria (Syntax.rs entry,
  syntax-decisions log, regenerated grammars, re-blessed snapshots) are all
  `verified`. It decided conversion *direction* only. Its option A text says
  "Text parsing remains `Type.parse`". It did not add or test an enum text
  parser, so there is nothing to duplicate here.
- D-ENUM-INGRESS1 (checked `from_int` for explicitly numbered enums) is still
  open and not ratified. It covers integer ingress only, not text.
- `node Tools/tower/tower.mjs decision show D-ENUM-PARSE1` returns
  `tower: no decision D-ENUM-PARSE1`, so no text-parse ballot exists yet.

## Method

Probe: `~/.cache/jet-dev/scratch/Closer11/enumtext/probe_heading.jet`. The
scratch root `~/.cache/jet-dev/scratch/Closer11/package.jet` grants IO and
Mem.Alloc. The probe declares one payload-free enum with plain cases, a variant
group (D-TAG1), and a case carrying `#Rename("west-side")`:

```jet
enum Heading {
    North
    South
    Diagonal {
        NorthEast
        SouthWest
    }
    #Rename("west-side") West
}
```

The probe tries two routes on the same ten inputs:

- (a) **Codable/JSON route**: `json.decode<Heading>(json.to_string(DataTree.Text(~text)))`.
  The text has to be quoted as JSON first.
- (b) **Hand-written route**: `fn parse_heading(text: String) -> Heading UnknownHeading!`.
  This is an `if text == { "North" -> ... else -> return Err(UnknownHeading{text: text}) }`
  table, with `#Error struct UnknownHeading { text: String }`.

Commands, run from `~/.cache/jet-dev/scratch/Closer11`:

```
~/.cache/jet-luna/safe-jet.sh run enumtext/probe_heading.jet
~/.cache/jet-luna/safe-jet.sh run --interpret enumtext/probe_heading.jet
~/.cache/jet-luna/safe-jet.sh build enumtext/probe_heading.jet && ./.jet/build/probe_heading
```

## Evidence (observed; identical on `jet run`, `--interpret` and AOT: `diff aot.out run.out` → identical)

```
wire North = "North"
wire NorthEast = "Diagonal.NorthEast"
wire West = "west-side"
display West = West
display NorthEast = Diagonal.NorthEast
[North] json: North | manual: North
[north] json: rejected ("north") | manual: rejected UnknownHeading(north)
[NORTH] json: rejected ("NORTH") | manual: rejected UnknownHeading(NORTH)
[NorthEast] json: rejected ("NorthEast") | manual: Diagonal.NorthEast
[Diagonal.NorthEast] json: Diagonal.NorthEast | manual: rejected UnknownHeading(Diagonal.NorthEast)
[Diagonal] json: rejected ("Diagonal") | manual: rejected UnknownHeading(Diagonal)
[West] json: rejected ("West") | manual: West
[west-side] json: West | manual: rejected UnknownHeading(west-side)
[Nowhere] json: rejected ("Nowhere") | manual: rejected UnknownHeading(Nowhere)
[] json: rejected ("") | manual: rejected UnknownHeading()
```

### Entry points and error contract (criterion 1)

- **There is no generated enum text entry point.** No `Heading.parse`,
  `from_name` or `from_str` is generated. Triage found none in
  `Compiler/JetSema`, `Compiler/JetCodegen/Source/Codegen/AutoDerives.jet` or
  `BuiltinStatics.jet`, whose parse rows cover only Int/Float/F64/PasswordHash.
- **Route (a)** is `json.decode<T>(String) -> T` with a Core encoding error.
  The probe handled it with `?? return`. It needs JSON quoting, and it matches
  the frozen **wire** name only.
- **Route (b)** is ordinary user code, `-> Heading UnknownHeading!`. The error
  type has to be `#Error`: the triage's suggested `Target String!` spelling is
  rejected with
  `E2417 Explicit failure domain String (text) is not an Error type`. Any
  ballot must use a Core or `#Error` error type.

### Known, unknown, grouped, case and rename behaviour (criterion 2)

| Input | JSON route (wire names) | Hand match (spelled names) | Python `E[name]` | Python `E(value)` |
|---|---|---|---|---|
| `North` | North | North | North | North |
| `north`, `NORTH` | rejected | rejected | KeyError | ValueError |
| `NorthEast` (leaf of a group) | rejected | Diagonal.NorthEast (because the author spelled it) | NorthEast | ValueError |
| `Diagonal.NorthEast` (qualified) | Diagonal.NorthEast | rejected (not spelled) | KeyError | NorthEast |
| `Diagonal` (group name, not a case) | rejected | rejected | KeyError | ValueError |
| `West` (source name of a renamed case) | **rejected** | West | West | ValueError |
| `west-side` (frozen wire rename) | West | rejected | KeyError | West |
| `Nowhere`, `` (empty) | rejected | rejected | KeyError | ValueError |

What this shows:

- Source name, wire name and display text are three separate things.
  - `#Rename` changes the wire only: encoding gives `"west-side"`, decoding
    accepts `west-side` and rejects `West`.
  - Display still shows the source name, `West`.
  - A grouped case's wire and display name is the qualified
    `Diagonal.NorthEast`. The bare leaf `NorthEast` is not accepted by the
    Codable route.
- Both routes are case-sensitive, and neither accepts a group name as a case.

### Peer comparison on the same inputs (criterion 3)

- **Python 3.13.13** (executed with
  `python3 ~/.cache/jet-dev/scratch/Closer11/enumtext/peer_python.py`; its
  output is the table above).
  - `Enum[name]` is an exact, case-sensitive source-name lookup that raises
    `KeyError`.
  - `Enum(value)` is a value (wire-like) lookup that raises `ValueError`.
  - Python keeps name and value lookup as two separate entry points.
- **Java `Enum.valueOf(Class, String)`**: documented behaviour; not run here
  because no JDK is on this machine (`which java` finds nothing).
  - It is an exact, case-sensitive match on the declared constant name.
  - Unknown text throws `IllegalArgumentException`.
  - It has no rename concept: Jackson `@JsonProperty` is a separate wire layer.
- **.NET `Enum.Parse` / `Enum.TryParse`**: documented behaviour; not run here
  because no `dotnet` is on this machine.
  - Case-sensitive by default, with an `ignoreCase` overload.
  - It also accepts numeric strings and comma-joined flag names. That means
    `"42"` parses to an undeclared value, which is the membership hazard EN-C29
    records.
  - `TryParse` returns `bool`.
- **Jet today** matches Java and Python's name lookup only through a
  hand-written table. Its only generated route is the wire route (like Python's
  `Enum(value)`), and that route needs JSON quoting.
- This card adds no parse alias, string-enum family or optional package.

### Defect found while probing

- An enum named `Target` breaks every tier. Minimal repro:
  `~/.cache/jet-dev/scratch/Closer11/enumtext/minA.jet`
  (`enum Target { North South }` + `print("display {Target.North}")`).
  - `jet run`: ICE `Cranelift cannot execute MIR function ...::run: MIR enum variant North is missing`.
  - `--interpret`: `E0956 MIR enum variant has no canonical row`.
  - `jet build`: internal compiler error at
    `crates/jet-codegen/src/Codegen/MIRRust.rs:21453:40`; no binary is
    produced.
  - Renaming the enum to `Heading` makes both tiers work (`minB.jet`,
    `probe_heading.jet`).
  - Expected: a user enum named `Target` is legal, or it is rejected by a
    registered diagnostic.

## Verdict

- **Evidence: PASS.** Every row above was observed on all three tiers, and the
  Python peer was executed. Java and .NET are documented behaviour only.
- **Gap confirmed:** no generated enum text parser exists. The owner-gated ballot
  below is drafted for Pip to file; this worker cannot write to Tower.

## Ballot draft: D-ENUM-PARSE1 "Generated checked text parse for payload-free enums"

**Question.** Should every payload-free enum get a generated, checked text
decoder so a CLI or config value needs neither JSON quoting nor a hand-written
table?

**Same program in each option.**

```jet
enum Heading {
    North
    South
    Diagonal {
        NorthEast
        SouthWest
    }
    #Rename("west-side") West
}
```

- **A: source-name parse (recommended).**
  - Generated `Heading.parse(text: String) -> Heading ParseError!`.
  - Case-sensitive, exact **source** names only.
  - Grouped cases use their qualified source path (`Diagonal.NorthEast`),
    matching Display and D-TAG1. The bare leaf `NorthEast` is rejected, and so
    is the group name `Diagonal`.
  - `#Rename` is ignored: `West` parses, `west-side` does not. The wire stays
    with Codable.
  - The error is the existing Core `ParseError` family (never `String`, per
    E2417), and it names the expected set.
  - It reuses the D-ENUM-INGRESS1 generator from #3146 for the case table.
  - Program: `h :: Heading.parse(arg) ?? return usage()`.
- **B: no generator.**
  - Document two routes: `json.decode<Heading>` for wire text, and a
    hand-written `if text == { ... }` table for source names (both shown in
    this probe).
  - Program: the probe's `parse_heading` above (about 8 lines per enum,
    drifts when cases are added).
- **C: parse through the shape (wire) projection.**
  - Generated `Heading.parse(text)` that accepts the Codable wire names:
    `west-side` parses and `West` does not.
  - This is JSON decode without the quotes.

**Trade-offs.**

- A:
  - Keeps name / wire / display distinct (D-SHAPE-ONE1, D-ONE-SHAPE1).
  - Follows the D-SHAPE-CONVERT1=A "`Type.parse`" direction.
  - Stays total over new cases.
  - Cost: one generated member per enum.
- B:
  - Adds nothing.
  - But beginners need JSON quoting or boilerplate, and hand tables go stale
    silently.
- C:
  - One accepted spelling for files and CLI alike.
  - But it conflates CLI text with the frozen wire, and a later `#Rename`
    silently changes CLI behaviour.

**Edge cases.**

- Empty text and surrounding whitespace are rejected, with no trimming.
- Letter case: exact, no fold. A case-insensitive helper would be a separate
  ballot.
- Numeric text is rejected, unlike .NET: integers go through D-ENUM-INGRESS1
  `from_int`.
- Payload-carrying enums get no `parse`. Calling it is a registered diagnostic
  naming Codable decode.

**Beginner path.** `Heading.parse(arg) ?? ...` with completion listing the valid
spellings.

**Expert path.** Codable `json.decode<Heading>` for wire text; a custom table
when aliases are wanted.

**Recommendation.** A. Implementation belongs on a new card after ratification.
