# The lexical space: reservations and three repairs

Status: proposal, 2026-09-01, revised after the review passes; element 8 of `whole-language-frame.md`. Independently adoptable; each ballot stands alone. Ballots: D-RAWSTR1 (recommended A), D-TRAILCOMMA1 (A), D-SEMI1 (A), D-MARKERSHAPE1 (B, the law stands), D-MARKERARGS1 (A). Nothing here is implemented.

## Executive summary

Lane A inventoried 230 lexical mechanisms and 79 silhouette rows and found a grammar that is mostly one thing: one callable form (D-CALLABLE-ONE1), one binding sigil pair (`::` and `:=`), one marker sigil (`#`) with one stacking law (D-MARK-STACK1), one dot-brace typed literal (`Regex{"\d+"}`, `Path{"…"}`, `.{ … }`), and one compile-time sigil (`@`) with three parser-disambiguated jobs. The first draft of this element over-claimed: it called `#Every("03:00")` an unchecked string when it is checked at compile time (E0926), it proposed literal forms that do not exist, and it reversed a ratified marker rule without evidence. The rival-family review caught all of that, and the ballots were redrawn.

What remains are three real repairs and one narrowing: no way to write an ordinary String without escapes; a trailing comma accepted in a list literal and refused in a parameter list (two E0003 errors at the closing bracket, real today); a ratified no-semicolon law (S6, E0373) that the lexer does not enforce; and one marker, `#HTML("page.html")`, that stores a path as text while Jet has a checked `Path{"…"}`. The reservation ledger records which sigils and prefixes are claimed and which are open, so a future spelling ballot starts from the ledger, not from a lexer read.

Score: mechanisms deleted 0; capabilities kept all; capabilities gained 3 (raw ordinary Strings, uniform trailing commas, an enforced semicolon law).

| today | proposed | ballots |
|---|---|---|
| ordinary String interprets `\` and `{}` | backtick literal | D-RAWSTR1 |
| trailing comma differs by list kind | trailing commas everywhere | D-TRAILCOMMA1 |
| `;` parses and runs | E0373 with the contextual edit | D-SEMI1 |
| the bracket marker law stands | keep D-MARK-STACK1 | D-MARKERSHAPE1 |
| `#HTML` stores a path as String | `#HTML` takes `Path` | D-MARKERARGS1 |

## The lexical ledger (real today)

| sigil or prefix | job | ratified by | evidence |
|---|---|---|---|
| `::` | immutable binding | D-BIND1 | `Syntax.rs` |
| `:=` | mutable binding | D-BIND1 | `Syntax.rs` |
| `#` | attached rule; `#[A, B]` for two or more | D-VERDICT-732-1, D-MARK-STACK1 | `markers.rs:1-16` |
| `@` | compile-time block, name, or fact | D-ONCE-AT1 | `markers.rs:6-8` |
| `-[…]>` | effect row on a callable | D-EFF1 | `Sema/Effects.rs:438-500` |
| `!` | error type in a signature; deny-only root | D-RESULT1, D-EFF4 | |
| `?` | optional type | D-OPT1 | |
| `??` | fallback | D-RESULT-DECON2 | `syntax-surface.jet:302` |
| `T{expr}` | field default | D-DEFAULT-SHAPE1 | `codable_default.jet` |
| `Name{"…"}` | checked text head | S8, D-CHECKED-TEXT1 | `text/regex.jet:6-13` |
| `.{ … }` | typed anonymous value | D-POLICY-WORD1 | `syntax-decisions.md:2023-2029` |
| `` ` `` | unclaimed | | `Lexer/mod.rs` has no rule |
| `r"…"`, `$"…"` | unclaimed | | |
| `;` | retired (E0373 ratified, not enforced) | S6 | `Lexer/mod.rs` accepts it |
| `_name` | unclaimed as a reservation; leading underscore is an ordinary identifier | | lane A |
| `__core_intrinsic` | compiler-only namespace | D-CORE-CALL1 | `core_call.rs` |

The phantom grammar lane A found, `Syntax::KW_SWITCH` whose value is literally `"if"` and a `Stmt::Switch` AST variant no parser path produces, is carded as a defect (#2512), not balloted; `Test`, `Todo`, and `Pure` are contextual identifiers, not keywords, and need no change.

## Repair 1: a raw ordinary String (D-RAWSTR1)

Real today: `Regex{"\d+"}` and `Path{"C:\Users"}` already pass their text to a domain grammar, so a regex or a path never needs doubled backslashes. An ordinary String still interprets `\` and `{}`:

```jet
template :: "C:\\tools\\{{name}}\\bin"       // today: four backslashes and doubled braces to store one path template
```

Proposed (option A): backtick is unclaimed; inside it nothing is an escape, braces are literal, there is no interpolation, and the literal may span lines. The one thing it cannot contain is a backtick; use an ordinary String for that.

```jet
template :: `C:\tools\{name}\bin`
query :: `
  select *
  from orders
`
print(template)     // C:\tools\{name}\bin
```

| option | spelling | why not |
|---|---|---|
| A (recommended) | `` `…` `` | |
| B | `r"…"`, `r#"…"#` | a prefix is a second literal grammar; the hash form is a third |
| C | `Raw{"…"}` | Raw is not a domain; a checked-text type with no check yields a value that is not a String |
| D | keep escapes | plain Strings with backslashes and braces stay painful |

Amends: S8 (interpolation) and S20 (escapes) additively: neither applies inside a backtick literal. Regex and Path are unchanged.

## Repair 2: trailing commas everywhere (D-TRAILCOMMA1)

Real today, verified (verification block `lexical`):

```jet
xs :: [1, 2, 3,]            // accepted
fn f(a: Int, b: Int,) Int -> a + b
// Error [E0003]: expected `)` between arguments, found a piece of quoted text   (two errors at the closing bracket)
```

Proposed (option A): every comma list accepts a trailing comma: parameters, arguments, list and map literals, constructor bodies, marker groups, effect rows, import lists, type arguments, and loop headers. `jet fmt` writes a trailing comma when the list spans lines and removes it when the list fits on one line; a comment after the last item counts as the multi-line form.

```jet
fn f(
    a: Int,
    b: Int,
) Int -> a + b
f(1, 2)          // single line: fmt removes a trailing comma
```

| option | rule | why not |
|---|---|---|
| A (recommended) | accept everywhere; fmt decides layout | |
| B | legal only when the bracket is on a later line | a line-position rule is a second grammar for one comma |
| C | collections accept, everything else rejects | two rules today is the defect |
| D | reject everywhere | keeps the two-line diff every peer removed; retires working syntax |

Amends: S3 (parameter grammar) and the E0003 recovery table.

## Repair 3: enforce the semicolon law (D-SEMI1)

Real today: `a :: 1; b :: 2` parses and runs (`explicit_semicolon.jet`, exit 0). S6 ratified E0373 for the retired spelling.

Proposed (option A): the ratified E0373 with a behavior-preserving edit: a line break when code follows on the same line, removal at line end. The parser keeps a statement boundary at the same position so one file reports once and parsing resumes at the next statement. Precondition: a census of the corpus for deliberate semicolons before the error lands.

```text
a :: 1; b :: 2
Error [E0373]: `;` is not a statement terminator in Jet
  fix (behavior-preserving): break the line
a :: 1
b :: 2
```

| option | rule | why not |
|---|---|---|
| A (recommended) | E0373 with the contextual edit | |
| B | a lint with the same edit; `policy.lints.deny` promotes | retires a ratified code to add a lint that says the same thing |
| C | fmt only | a never-formatted file keeps two spellings |

Amends: nothing; S6 is enforced as written.

## The marker law stands (D-MARKERSHAPE1)

D-MARK-STACK1=A (2026-07-23, card #761): one attached rule is `#Rule`; two or more distinct rules share one `#[A, B]` list; a one-item list or an adjacent bare stack is E0999 with the canonical rewrite; repeated rules such as two `#Post` markers are allowed; field and variant sites take only the bracket form. The first draft proposed stacking bare markers; the rival review found no new evidence that meets the bar for reopening. The ballot is a challenge and its recommendation is B, the law stands.

```jet
#[Pre(cents > 0, "positive"), Post(result > cents, "grows")]
fn add_fee(cents: Int) Int -> { return cents + 5 }
#[Pre(cents > 0, "positive")] fn f(cents: Int) Int -> cents     // E0999: one-item list; fix: #Pre(...)
```

| option | what |
|---|---|
| A | Amend: distinct bare markers may stack on separate lines; the bracket list stays as optional grouping; field and variant sites also accept bare markers; `jet fmt` writes stacked lines for functions and types and the bracket form for fields. Amends D-MARK-STACK1=A and S82. Rejected: two accepted layouts for one marker set, and a reopening without new evidence. |
| B (recommended) | Keep D-MARK-STACK1 as ratified: one bare rule, one list for several, E0999 with the rewrite otherwise. The audit found no new evidence that meets the bar for reopening card #761. |

## One marker argument (D-MARKERARGS1)

The first draft claimed every marker string hides a grammar. It does not: `#Every("03:00")` is checked at compile time (E0926), `#Policy` takes typed `PolicySetting` values, and `#[Rename]`, `#[Env]`, and `#Unsafe` carry text on purpose. One marker remains: `#HTML("page.html")` stores a file path as a String while Jet has a checked `Path{"…"}`.

```jet
#HTML(Path{"page.html"})          // proposed (option A): a malformed path fails at compile time with the Path grammar's diagnostic
fn page() HTML -> { … }
#HTML("page.html")                // option B: no change; checked only when the file is missing
```

| option | what |
|---|---|
| A (recommended) | `#HTML` takes `Path`: the companion page argument is a checked `Path` value, and a malformed path fails at compile time with the Path grammar's diagnostic. Signature change in `Prelude/Markers.jet` (D-MARKSIG1): `HTML(page: Path)`; `jet fix` rewrites `#HTML("x")` to `#HTML(Path{"x"})`. |
| B | Keep the String: no change. Rejected: a path stored as text is checked only when it is missing. |

Amends: D-MARKSIG1's signature row for `#HTML`.

## Rungs

| repair | what the beginner types less | diagnostic |
|---|---|---|
| raw String | escapes for backslashes and braces | one diagnostic with one safe edit |
| trailing comma | one comma rule | E0003 |
| semicolon | one terminator rule | E0373 |

## Three exits

| exit | spelling |
|---|---|
| see | `jet explain E0373`, `jet explain E0003` print the rule and the edit; the ledger table above is rendered from `Syntax.rs` |
| write | the canonical spelling each diagnostic names |
| refuse | not applicable; no inference is introduced |

## Implementation shape

| phase | work |
|---|---|
| A | phantom `switch` grammar deleted (#2512); the semicolon census |
| B | none owed |
| C | after each ballot independently: backtick literal in the lexer with S8 and S20 amended; trailing comma in the parser with fmt rules and E0003 snapshots; E0373 with its edit and snapshot; `#HTML` signature change with one example |

## Where Jet loses today

| peer | what it does better | Jet today |
|---|---|---|
| Rust | accepts a trailing comma everywhere and has a raw or multi-line string form | typed text heads beat Rust for regexes and paths and lose on a plain path template |
| Go | accepts a trailing comma everywhere and has a raw or multi-line string form | typed text heads beat Go for regexes and paths and lose on a plain path template |
| Zig | accepts a trailing comma everywhere and has a raw or multi-line string form | typed text heads beat Zig for regexes and paths and lose on a plain path template |
| Swift | accepts a trailing comma everywhere and has a raw or multi-line string form | typed text heads beat Swift for regexes and paths and lose on a plain path template |

## Strongest unverified assumption

That the corpus contains no deliberate semicolon that carries meaning. The lexer accepts it today; the census in phase A is the proof, and it runs before E0373 lands.

| assumption | how it is proved | where |
|---|---|---|
| the corpus contains no deliberate semicolon that carries meaning | the census in phase A before E0373 lands | card #2508 |
