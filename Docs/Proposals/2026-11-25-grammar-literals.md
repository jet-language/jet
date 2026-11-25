# Grammar literals: design (card #7036)

Status: reviewed and repaired by ReviewGrammar on 2026-11-25 (§17). Every
repair is marked with its finding number, for example (R-C1). Line numbers
were read again at dev `e1d1b579653`. The first draft cited `4b48f2bc485`.

## Summary for the owner

You write a grammar once, as text, in an ordinary checked literal:
`Ini: Grammar{"""…"""}`. At build time Core's grammar compiler reads it, and:

- each parser rule becomes a Jet type;
- each type gets `parse`, with `line:column` errors, and `print`;
- a broken grammar fails the build. So does an ambiguous one, or one that is
  left-recursive in any form other than the operator form. The error names the
  rules and gives a sample input;
- the parser is plain Jet code with no tables and no backtracking. Before any
  hand-written parser is deleted, a paired benchmark must show the generated
  one is at least as fast.

The rule notation is the one most parser generators share. It uses the
ANTLR 4 and Bison rule form `name : … ;`, quoted literals, `|`, `* + ?`,
parentheses, `[a-z]` classes and `//` comments. Jet adds only one spelling of
its own, the hole marker `HOLE<slot=…>` (§3).

The same grammars check the text heads at build time: `SQL`, `HTML` and `Sh`
(which accept any text today), plus `URL`, `DateTime`, `Path` and `Regex`.
Each hole gets the type and the escaping of its position. In SQL, a table
name is quoted and a value is bound. One SQL grammar replaces Core's three
SQL readers.

The owner approved these as part of P31 on 2026-11-25:

- the declaring form `Name: Grammar{…}`;
- the standard notation;
- the `CheckedText` change;
- the Core surface names;
- the Sh and HTML refusals.

The review's two follow-ups were decided as the safe options under that
approval (§14):

- **Sh:** a hole that names the program must be a `Path`. A bare newline, and
  `$` or a backtick inside double quotes, are build errors.
- **HTML:** holes inside `on*`, `style`, `srcdoc` and `srcset` attributes are
  build errors.

---

## 1. Sources this design rests on

All paths are relative to the repository root.

| Fact | Where |
|---|---|
| `CheckedText` trait as declared: `Error: type`, `check: fn(text: String) -[]> Error!`, `encode_hole: fn<T: Printable>(value: T) -[]> String`. `Hole` and `build(parts, holes)` are described in its comment and checked by sema, not declared (R-L1) | `Compiler/Prelude/Core/prelude.jet:57-69` |
| A building head's `check` sees the parts joined with `{}`. The check is skipped when `Error` is `Never` (SQL, HTML, Sh today). A distinct-String head with non-constant holes is checked at run time | `Compiler/JetSema/Source/Sema/Expressions/Constructors.jet:2023-2177` (`sema_expression_checked_text_literal`, `sema_expression_checked_text_runtime`) |
| `Type.from(text)` and the `#Unsafe` `raw` path call the same `check(text)`. `check` must return `Result<Unit, Error>` (E0907) | `Constructors.jet:1862-2021` (`sema_expression_checked_text_from`, `sema_expression_checked_text_raw`) |
| Further sema sites read the `check(text)` contract | `Compiler/JetSema/Source/Sema/Registration/OwnedTraitContracts.jet:432-439`; `Sema/Effects/Declarations.jet:367`; `ComptimePrecheck.jet:1154`; `TypeRelations.jet:2431-2452`; `Compiler/JetSemIndex/…/Symbols.jet:295`; `GateLedger.jet:90-104` |
| D-TEXTHEAD-TYPE1=A: "the same pure `check(text)` function runs at compile time for known literals and at runtime for `Type.from(text)`". It rejected "a dedicated declaration mechanism for behavior ordinary types and traits can already model" | Tower `decision show D-TEXTHEAD-TYPE1` |
| D-TEXTHEAD-HOLES1=A: `check` sees `{}`-joined text, and "no literal can fail its check while the program runs" | Tower `decision show D-TEXTHEAD-HOLES1` |
| The check is a `SemaComptimeRequest` (kind `CheckedText`) that runs natively in the compile-time child. Only pass or fail comes back. The item kinds are Constant, MarkerArgument, FieldDefault, TypeLevel, CheckedText and BodyExpression | `Constructors.jet:2023-2177`; `Compiler/JetSema/Source/Sema/ComptimeFunctions.jet:32` |
| E2712 `Jet.CheckedTextRejected` is declared in the Prelude diagnostics. `DiagnosticRows.jet` in Foundation is a generated copy. A rejected literal stops the build (`flushed.stop`). The reason is sniffed from `String`/`reason`/`message` fields | `Compiler/Prelude/Diagnostics.jet:23288`; `Tools/Agent/DiagnosticRows.jet:1-38`; `Compiler/JetDriver/Source/Driver/Pipeline.jet:3338-3349, ~3378` |
| A diagnostic is registered by adding a type to `Compiler/Prelude/Diagnostics.jet` and running `jet Tools/Agent/DiagnosticRows.jet --write`. `--check` fails for an active code that no source line reports (`UNREPORTED_CODES` may only shrink). Core code reports a code through `impl Diagnostic { code: "E…" }` | `Tools/Agent/DiagnosticRows.jet:1-38`; `Core/Time/Time.jet:202-212`; `Core/Prelude/Prelude.jet:279` |
| Regex has its own path, which runs `core.regex.compile` inside the compiler process, and its own code, E0152 | `Constructors.jet:2340-2393`; Regex source `Core/Regex/Regex.jet` (14 `parse_*` methods at 706-1066) (R-H7) |
| Boundary heads are a list of spellings that the loader uses for Core demand loading | `Compiler/JetFoundation/Source/Facts/Checked.jet:134`; `Loader.jet:4589` |
| SQL head: `Error: Never`, `Hole: DBValue`, `check` accepts everything. `encode_hole` binds any type it does not list (lists included) as its display text | `Core/DB/DB.jet:325-346` |
| HTML head: a run-time state machine picks the escaping. `&` is already escaped by `text_escape` | `Core/Text/HTML.jet:115-205` |
| Sh head: `build` splits on whitespace (`text.words()`), newlines included | `Core/Process/Process.jet:142-164` |
| URL head: a nonce marker stands in for each hole and the text is parsed again | `Core/Net/URL.jet:171-298` |
| DateTime head: refuses holes, checks with `parse_rfc3339`, and parses again in `build` | `Core/Time/Time.jet:1182-1199` |
| Path head: refuses NUL. `build` encodes each hole with `path_component_encode` | `Core/Files/Files.jet:60-80` |
| Two more SQL readers: policy scoping and the analytics subset | `Core/DB/DB.jet:404-724, 3419-3460`; `Core/DB/SQL.jet` (1,031 lines), used by `Core/DB/Query.jet:49,61` |
| Engine SQL parser: a port of SQLite 3.45.0's Lemon LALR(1) parser | `Core/DB/Engine/Parse.jet` (2,162 lines), `ParseTables.jet` (7,017), `Tokenize.jet` (1,162) |
| Marker `expand` templates allow only a fixed set of static methods | `Compiler/JetSema/Source/Sema/Registration/Derives.jet:421` |
| Declared text blocks are statement-level `ScopeMember{dsl: true}`, and their body is parsed as Jet statements | `UserTemplates.jet:901-914, 1245-1256` |
| **A root `#loop` waiting on a `#run` value costs a full extra compile.** When `loop_values` is non-empty, the driver runs a whole probe compile, loads again, then compiles for real. The waiting check runs over every loaded module | `Pipeline.jet:~5959-5971` (`jet_driver_compile_stages`); `UserTemplates.jet:247`; `Sema/Registration/Items.jet:733-741`; `Tests/Cases/ComptimeInputs/comptime_root_loop_waits_on_run_value.case` (R-C1) |
| Core derive expansions are recorded in the Core bodies file and read back by a compile bound to that Core | `Sema/CoreBodies.jet:960-1052`; `Items.jet:727-732` |
| `b.generate`, `meta.add` and `b.compiler` work only in the root package build (E0776). D-META-LADDER1: "adding a type from text checks Generate, Types and Raw together". `Meta` and `MetaCode` are expert effects | `Docs/Spec/spec.md:7563-7597`; `Docs/Spec/syntax-decisions.md:4534`; `Core/Compiler/Meta.jet` |
| D-GENERIC-TYPEFN1 (type recipes) was archived on 2026-10-29 | Tower `decision show D-GENERIC-TYPEFN1` (`retiredAt`) |
| `Name{"…"}` is the checked text head; there is no `Grammar` row. `Name: …` at module scope is classified by meaning, not by casing. Sema casing wants SCREAMING_CASE for a module constant | `Compiler/JetFoundation/Data/Syntax/Spellings.jet:77, 260-299`; `Compiler/JetParser/Source/Parser/Declarations/Types.jet:54-63` (#6176) |
| `{{`/`}}` are literal braces; checked bodies own their backslashes | `Docs/Spec/spec.md:76, 88-92` |
| spec.md:97 reads "in text `^`, `<` and `>`", but the code escapes `&` | `Docs/Spec/spec.md:97`; `Core/Text/HTML.jet:186-189` |
| Compile-time step limit: 10,000,000 per compile-time call (E0952, `#StepLimit(n)`) | `Docs/Spec/Reference/metaprogramming.md:180-220` |
| A new Core module needs a layer in `Core/package.jet` and `module`/`source_module` rows in `Compiler/Prelude/Core.rows` | `Core/package.jet:7-12`; `Compiler/Prelude/Core.rows:196-197` |
| `Tests/Cases/Grammar/` already holds the cases for Jet's own grammar and the editor-grammar guards | `Tests/Cases/Grammar/` (R-H7) |
| Test selection: `jet test Compiler --profile <p> [FILTER]` or `--filter X`. A row is selected when its ID, relative to the repository root, contains X. A filter that selects nothing fails. `--profile core --filter X` runs only the Core files holding a matching row | `Docs/Spec/Contributing/examples.md:65-96`; `Docs/Spec/Reference/cli.md:1270-1298`; `Compiler/JetTestKit/Source/Exec.jet:53-67` |
| `jet self devtools grammars` takes no `--check`. It rewrites the generated editor sections; the guard is a case | `Compiler/JetCLI/Source/CLI/Front.jet:3370`; `EditorGrammars.jet:118`; `Tests/Cases/Grammar/editor_grammars_match_generated_sections.case` |
| Paired performance cell and compile-throughput gate | `Tools/Perf/ZeroCostPair.jet:1-21`; `Tools/Gauntlet/measurement-manifest.json:332` (`zero_cost_pairs`); `Tools/Perf/CompileThroughput/Throughput.jet:1-94` |

## 2. The mechanism

### 2.1 What Jet can do today, and the gap

The card needs types and functions made from a literal, in library code, at
build time. Each existing mechanism was checked:

| Mechanism | Can it run a grammar compiler? | Can it make nominal types? | Verdict |
|---|---|---|---|
| Checked text heads (`CheckedText`) | Yes. `check` is Core code run natively in the compile-time child | No. Only pass or fail comes back | Has half the job |
| `#run` compile-time code | Yes | No. Values only; type recipes were retired | No |
| Marker `expand` templates | No. The expander allows only a fixed set of static methods | Yes, module-level items with pasted names | Cannot compute the types |
| Declared text blocks `#Name { … }` | No | Statement-level only | The body is Jet statements |
| Root `#loop` over a `#run` value | Yes, through a whole-program probe compile | Template items only. Templates have no field or case loops and cannot emit recursive parse code | The nearest existing route. Too weak, and its probe compile breaks the compile budget (R-C1) |
| `meta.add_struct`, `b.generate` | Yes | Yes | Root package only; an expert effect (D-META-LADDER1) |

**Finding:** no existing mechanism lets library code turn a literal into
nominal types with computed fields and recursive parse functions. Something
new is needed. It must be the smallest join of two existing halves:

- the checked-text check, which already runs Core code at build time;
- generated items, which already enter sema through the same admission as
  marker output (`sema_user_rule_accept`).

The owner approved the declaring head (B1) in P31. That approval supersedes,
for this one head, D-TEXTHEAD-TYPE1's rejection of a dedicated declaration
mechanism. It also covers this as the one sanctioned way, outside the root
package, to add types from text (D-META-LADDER1). G-C2 records both
amendments in `syntax-decisions.md` (R-H1).

### 2.2 The chosen mechanism: a declaring checked head

1. **`core.grammar.Grammar` is an ordinary `CheckedText` type.**
   - Its `check` is the grammar compiler, written in Core Jet in
     `Core/Grammar/`. It runs natively in the compile-time child, as every
     head check does, so it has one meaning on every tier (I9).
   - `check` returns `Checked<GrammarPlan, Never>`. A `GrammarPlan` is plain
     data made only from the compile-time value algebra: `Int`, `Float`,
     `Bool`, `Char`, `String`, enums, lists and structs. That excludes maps
     and handles (`Docs/Spec/Reference/metaprogramming.md:86-94, 976-983`).
     It holds the rules, type shapes, decision tables, byte classes, the
     keyword hash (as lists) and the print table.
2. **The declaring binding is found by symbol identity, never by spelling**
   (R-H1).
   - A module-level binding `Name: Head{"""…"""}` declares when `Head`
     resolves, through the module's `use` table, to `core.grammar.Grammar`.
   - This holds under aliasing. A user type that happens to be named
     `Grammar` never declares.
   - There is no `DECLARING_TYPED_HEADS` list. `Grammar` is never
     demand-loaded; a module that declares a grammar writes
     `use core.grammar…`.
3. **Staging: one pre-registration batch, not the root `#loop` probe
   compile** (R-C1).
   - A `Grammar` literal is closed. It has no holes, and its check reads only
     its own text and `core.grammar`.
   - So after loading, and before registration, the driver gathers every
     declaring literal in the loaded graph. It sends them to the compile-time
     child as one batch. The child compiles only the `core.grammar` closure,
     and the plans come back as compile-time values.
   - Registration then runs once, with each module's plans in its input. This
     is a new `grammar_plans` field beside `loop_values` in
     `SemaRegistrationModuleInput`.
   - The program is never compiled twice.
4. **Core's own grammars come from the Core bodies record** (R-C1).
   - This is the existing pattern for Core derive expansions
     (`CoreBodies.jet:960-1052`).
   - The Core library's check keeps each Core module's plan and generated
     items under a new record key. A compile bound to that Core reads them
     from the record.
   - A program that uses `SQL{…}` therefore never runs the SQL grammar
     analysis.
   - Whether the performance gate counts this toolchain record as a "compile
     cache" is the same question it already answers for Core bodies and
     derive records [UNVERIFIED: no ruling found; §16].
5. **One thin builder turns a plan into typed items.** It lives in
   `Compiler/JetSema/Source/Sema/Registration/GrammarItems.jet` and makes:
   - structs and enums;
   - a `<Start>Slot` enum when the grammar has holes;
   - rule-id constants;
   - `impl` blocks for `parse`, `print`, `print_into`, `parse_into` and
     `check_parts`.

   Rules for the builder:
   - It builds `Item` and `Expr` trees directly, never source text
     (D-META-CODE1). The items pass through `sema_user_rule_accept` and are
     checked like hand-written code (I3).
   - **The builder only arranges calls** (R-H1). Every meaning lives in
     `Core/Grammar/Runtime.jet`: the cursor, the class scans, keyword lookup,
     error text, resync and the depth bound. No error text, policy or limit is
     written in the compiler (I9).
   - Generated items take the binding's visibility. A `priv` binding makes
     `priv` types. Core's head grammars are `priv`, so the roughly 400 SQL rule
     types never join Core's public surface (R-H9).
   - A diagnostic inside a generated item points at the grammar literal.
     Helpers are named under the compiler-only `__` namespace.
6. **The grammar text is the single source of truth.** The types, the parser,
   the printer and the syntactic hole slots all come from it. The heads add
   policy only, such as the escaping for each slot, in their own Core code.

Why this and not the alternatives:

- **Root `#loop` + `#run` + templates:** templates have no field or case loops
  and cannot emit recursive functions. Each use also costs a whole-program
  probe compile.
- **Marker or template expander widened:** this would be a second compile-time
  interpreter (I9).
- **`meta.add_struct` lifted into library code:** contradicts D-META-LADDER1
  for every library, not just one reviewed Core compiler.
- **Emitting Jet text:** forbidden by D-META-CODE1.
- **A run-time table interpreter:** slower than direct code, which risks the
  performance gate.
- **User-written types plus mapping attributes:** the rules would be written
  twice.
- **A generic `Node` tree:** contradicts "generates the result types".
- **Keeping Regex's in-process path:** two mechanisms for one meaning (I8).

### 2.3 The CheckedText change (B3, owner-approved)

```jet
CheckedText: trait {
    Error: type                       // as today; may be a list whose items implement Diagnostic
    Hole: type{String}                // as today
    Slot: type{TextSlot}              // new: the kind of position a hole sits in
    Shape: type{[String]}             // new: what check learned; embedded as constant data
    check: fn(parts: [String]) -[]> Checked<Shape, Slot> Error!
    accepts: fn(slot: Slot, hole: TypeId) -[]> String?     // new; default: accepts every type
    encode_hole: fn<T: Printable>(value: T, slot: Slot) -[]> Hole
    build: fn(shape: Shape, holes: [Hole]) -[]> Owner
}
Checked: struct<Shape, Slot> { shape: Shape, slots: [Slot] }   // Prelude
```

How each part is used:

- **`check` takes the parts**, never joined text. A head grammar is parsed with
  a hole between parts, so the `{}` ambiguity and the URL marker trick both go
  away.
- **`slots[i]`** is the position of hole `i`. The grammar gives the syntactic
  slot. The head's `check` may refine it, for example by the attribute name in
  HTML.
- **`accepts`** runs at build time for each hole, given the slot and the hole's
  type id. A reason is reported as `Jet.CheckedTextHoleSlot` at the hole's own
  span.
- **`Shape`** is computed once at build time. It is embedded through the
  existing `BodyExpression` compile-time route, so it must be data a
  compile-time value can hold (R-M5). `build` never parses again at run time.
- **`Error` may be a list.** Each item that implements `Diagnostic` is
  reported under its own registered code, at the span its `at: TextSpot`
  maps to. This replaces the field sniffing in `jet_driver_checked_text_reason`.

Paths the first draft left undefined (R-C2):

- **`Type.from(text)`** calls `check([text])`. With no holes, an empty `slots`
  is the only valid result, so a text kind still has one pure check, as
  D-TEXTHEAD-TYPE1 requires.
- **`Type.raw(text)`** (`#Unsafe`) keeps skipping the check.
- **A distinct-String kind with non-constant holes** encodes the holes, joins
  the text, and calls `check([joined])` at run time, as it does today.
- **E0907** now reads "`{type}.check` must return `Checked<Shape, Slot>` with
  the type's `Error`".
- **The check always runs.** An `Error: Never` head no longer skips it,
  because `check` now produces the shape. SQL, HTML and Sh gain real `Error`
  types.
- A text kind pays the cost the owner accepted: it writes `check(parts)` and
  returns `Checked{shape: parts.copy(), slots: []}`. The Prelude defaults
  `Shape` to `[String]` and `Slot` to `TextSlot`, so that is the only change
  such a kind needs.

### 2.4 Positioned and collect-all errors

- Sema records each part's span and the dedent map of a triple-quoted body. A
  `TextSpot{part, offset}` becomes an exact `line:column` in the source.
- The driver stops setting `flushed.stop` after E2712. It follows the
  existing `graph.keep_going` pattern of `jet_driver_comptime_discriminant_range`,
  so every malformed literal is reported in one build (R-M2).
- Inside one literal, the generated parser resynchronizes at the nearest
  enclosing repeat's separator or closing literal. It reports at most one
  error per resync point.

## 3. Rule notation (standard EBNF; owner-approved)

The owner asked for the notation that mainstream parser generators share,
not a Jet invention. The table gives each form and the tools that already
spell it so. Sources:

- ANTLR 4 parser and lexer rules: <https://raw.githubusercontent.com/antlr/antlr4/master/doc/parser-rules.md>
  and <https://raw.githubusercontent.com/antlr/antlr4/master/doc/lexer-rules.md>
- ANTLR 4 left recursion: <https://raw.githubusercontent.com/antlr/antlr4/master/doc/left-recursion.md>
- GNU Bison rule syntax: <https://www.gnu.org/software/bison/manual/html_node/Rules-Syntax.html>
- Lark grammar reference: <https://lark-parser.readthedocs.io/en/latest/grammar.html>
- W3C XML 1.0 §6 Notation: <https://www.w3.org/TR/xml/#sec-notation>
- pest syntax: <https://pest.rs/book/grammars/syntax.html>

**Rule-definition form: `name : alternatives ;`.**

- It is the form of ANTLR 4 (`retstat : 'return' expr ';' ;`) and of
  yacc/Bison (`result: components…;`).
- Lark uses the same `name: …` without the semicolon.
- The other forms are `::=` (W3C, Lemon) and `=` (ISO 14977, pest). Each has
  fewer mainstream users. `=` would also clash with ANTLR's label `=`.
- The `;` terminator makes rules independent of line breaks and indentation.

| Form | Meaning | Spelled this way by |
|---|---|---|
| `expr : … ;` | A parser rule. Its name starts lowercase, and it becomes a type named in PascalCase (`key_value` → `KeyValue`), as ANTLR names its context classes | ANTLR 4, Bison, Lark |
| `ID : … ;` | A token rule. Its name is ALL_CAPS. It is matched as text, with no skipping inside | ANTLR (uppercase initial), Lark (all caps) |
| `fragment DIGIT : [0-9] ;` | A helper usable inside tokens; not a token itself | ANTLR 4 |
| `WS : [ \t]+ -> skip ;` | A skipped token (spaces, comments) between the parts of parser rules | ANTLR 4 |
| `'text'` or `"text"` | A literal; both quote styles are accepted | W3C EBNF and ISO 14977 (both); ANTLR (`'…'`); Lark and pest (`"…"`) |
| `'select'i` | An ASCII case-insensitive literal | Lark |
| `[a-z_]`, `[^"\\]`, `\p{L}` | Character classes, negated classes, Unicode properties. Escapes `\n \r \t \\ \] \-` and `\u{…}` | W3C EBNF and Lark (`[^…]`); ANTLR (`\p{…}`, `\u{…}`) |
| `.` | Any one code point (tokens only) | ANTLR 4 lexer |
| `a b` | Sequence | all |
| `a \| b` | Alternatives. Unordered: the choice must be decidable (§6) | all |
| `( … )` | Group | all |
| `x?`, `x*`, `x+` | Optional and repeats | all |
| `.*?`, `.+?` | Non-greedy repeat; tokens only, and it must be followed by a literal | ANTLR 4, regex |
| `label=elem` | Captures `elem` into a field | ANTLR 4 |
| `label+=elem` | Appends `elem` to a list field | ANTLR 4 |
| `… # Name` | Names the alternative; it becomes the enum case `Name` | ANTLR 4 |
| `expr : left=expr '+' right=expr # Add \| … ;` | Operator precedence by direct left recursion. Earlier alternatives bind tighter, and `<assoc=right>` marks right associativity (§3.1) | ANTLR 4 |
| `import Base ;` | Reuses another grammar of the same module. Its rules are overridden by rules of the same name here | ANTLR 4 (`import`) |
| `EOF` | End of input | ANTLR 4 |
| `// …`, `/* … */` | Comments | ANTLR 4, Lark (`//`), W3C (`/* */`) |
| `HOLE<slot=Name>` | **The one Jet addition**: a head hole may sit here, in slot `Name`. It borrows ANTLR's element-option syntax `<name=value>` (`<assoc=right>`, `<fail=…>`) | ANTLR 4 element options |

Not supported, each with a registered error and a fix:

- ANTLR `{…}` actions and `{…}?` semantic predicates, because `{` opens a hole
  in a checked body. Use code over the tree, or a sink.
- `options {…}`, lexer `mode`, and PEG `!`/`&` predicates.
- Back-references and `%` separators. Matching close tags is a check over the
  tree (§8.5). A separated list is spelled `items+=x (',' items+=x)*`.

A literal brace is written `'{{'`/`'}}'`, as in every checked body
(spec.md:76).

### 3.1 Operator precedence (the ANTLR 4 form)

A rule may be directly left-recursive in the operator form, as ANTLR 4
defines it:

- **Binary:** the rule is the first and last element (`left=expr '*' right=expr`).
- **Suffix:** the rule is first but not last (`operand=expr '(' … ')'`).
- **Prefix:** the rule is last but not first (`'-' operand=expr`).
- **Precedence** is the order of these alternatives: the first binds
  tightest. `<assoc=right>` makes one alternative right-associative.
- **Same level:** to put several operators on one level, use one alternative
  with a labeled set: `left=expr op=('*' | '/') right=expr # Mul`.
- **A prefix operator** may begin any operand. Its own operand binds at its
  own level, so `-2^2` is `Neg(Pow(2, 2))` when `Pow` is listed before `Neg`.
- **A middle operand** sits between two literals of one alternative
  (`expr 'BETWEEN'i expr 'AND'i expr`). It is parsed at the level just above
  any binary alternative whose operator is the next literal (`AND`), and at
  the lowest level otherwise. This is what SQLite's `%left`/`%prec`
  declarations achieve for `BETWEEN`. ANTLR leaves it to the author (R-H4).
- **Any other left recursion is a build error that names the cycle:** indirect
  recursion (`a → b → a`), or a left-recursive alternative of no operator
  form. ANTLR reports mutual left recursion the same way.

### 3.2 Keywords

- **Contextual lexing:** at each decision, only the tokens viable there are
  tried, as in Lark's contextual lexer.
- **Literals win ties:** when a word-shaped literal and a token class are both
  viable and match the same text, the literal wins. This is ANTLR's and
  SQLite's keyword priority.
- **Fallback is explicit**, as in the ANTLR grammars-v4 SQLite grammar. Its
  `any_name : IDENTIFIER | fallback | STRING_LITERAL | …` and `fallback : …`
  rules are at `sql/sqlite/SQLiteParser.g4:776-889` (read 2026-11-25). A name
  position lists the keywords allowed as names:
  `name : ID | fallback_keyword ;` with `fallback_keyword : 'ABORT'i | 'ACTION'i | … ;`.
  For SQL, the list is the port's `yyFallback` table (§9.1).
- **Reserved words are rejected by the head, not the lexer.** A name position
  where no keyword is viable lexes `select` as `ID`. SQLite rejects that, so
  the SQL head's check rejects a bare name spelled like a keyword its
  `fallback_keyword` rule does not list. Both lists are read from the plan,
  never copied (R-H3).

## 4. Generated types and members

Rules:

- Every parser rule becomes a type in the declaring module, in PascalCase,
  with the binding's visibility.
- The binding name must be the PascalCase name of a rule. That rule is the
  start rule, and `parse` requires `EOF` after it. A type-name clash is the
  ordinary duplicate-declaration error.
- A rule with one alternative becomes a `struct` of its fields.
- A rule with several alternatives becomes an `enum`. Following ANTLR,
  either every alternative is labeled `# Name` or none is.
  - **Labeled:** case `Name{fields}`. If the alternative is exactly one
    unlabeled rule reference, the case is `Name(Rule)`. If it has no fields,
    it is a unit case.
  - **Unlabeled:** every alternative must be exactly one rule reference, and
    the case is `Rule(Rule)`.
- Fields (as ANTLR's accessors):

  | Element | Field |
  |---|---|
  | `label=TOKEN` | `label: String` |
  | `label=rule` | `label: Rule` |
  | Unlabeled `rule` that occurs once in the alternative | `rule: Rule` (snake_case name) |
  | `label='lit'?` | `label: Bool` (present or not) |
  | `label=('a' \| 'b')` | `label: String` |
  | `x?` | `T?` |
  | `label+=x` (anywhere, any count) | `[T]` |
  | Groups `( … )` | flattened |

- An unlabeled token or literal carries no field. The printer writes the
  token's canonical text, which is its first literal alternative.
- A rule referenced twice in one alternative without labels is
  `Jet.GrammarFieldShape`, asking for labels.
- Recursive rules become recursive enums, which Jet boxes invisibly
  (`Examples/Types/RecursiveEnum.jet`). A struct that holds itself, such as
  `list : head=item (',' rest=list)? ;`, is [INFERENCE]. G-A11 makes the
  builder emit such a rule as a single-case enum if structs cannot recurse.

Members on each rule type `R`:

| Member | Signature |
|---|---|
| `parse` | `R.parse(text: String) -> R GrammarError!` |
| `print` | `r.print() -> String GrammarError!` and `r.print_into(^out: [U8]) -> Unit GrammarError!` (R-H5) |
| `parse_into` | `R.parse_into<S: GrammarSink>(text: String, ^sink: S) -> Int GrammarError!`. Allocation-free. It emits `open(rule)`, `close(rule)` and `token(kind, start, end)` events, and returns the end offset |
| `check_parts` | `R.check_parts(parts: [String]) -> Checked<[String], <Start>Slot> [GrammarError]!`. Only on grammars that use `HOLE`; the head impls call it |
| Constants | `R.RULE: Int`, and token-kind constants for sinks |

Core types in `core.grammar` (B4, owner-approved): `Grammar`, `GrammarPlan`,
`GrammarError{line, column, offset, expected: [String], found: String}` (shown
as `3:14: expected "]", found end of line`), `GrammarSink`, `TextSpot`, and
`Checked` (Prelude).

The print law: for every value `v` built by `parse`,
`R.parse(v.print()?) == v`.

- **The printer is canonical.** It writes the separator `skip` allows: one
  space only where two adjacent tokens would lex as one. Comments and spacing
  are not kept.
- **`print` checks each token field against its token rule** (R-H5). A
  hand-built `Entry{name: "a\n[admin]"}` would otherwise print a new INI
  section, which is injection by printing. Such a value returns a
  `GrammarError` that names the field. Each check is one linear scan of the
  field.
- **Parentheses** (R-H5). A hand-built value that breaks precedence, such as
  `Mul{left: Add{…}, …}`, is printed through the grammar's grouping
  alternative. That is the operand alternative of the form
  `open label=rule close`. When the grammar has none, `print` returns a
  `GrammarError`.

## 5. Errors and diagnostics

Each code is declared as a type in `Compiler/Prelude/Diagnostics.jet`, with
what/why/fix text and a UI snapshot (I4). Its copies are regenerated with
`jet Tools/Agent/DiagnosticRows.jet --write`.

Each code lands in the same item as its first reporter, because
`DiagnosticRows.jet --check` refuses an unreported active code (R-H7). The
Core error types in `core.grammar` report them through
`impl Diagnostic { code: "E…" }`, the pattern `Core/Time/Time.jet:202-212`
uses. Build-time grammar errors are collected and reported in one build.

| Code name (new unless noted) | When | Points at |
|---|---|---|
| `Jet.GrammarNotation` | The notation is malformed. This includes an unclosed class, a missing `;`, an action `{…}`, `options`, `mode` or a predicate | `line:column` in the literal |
| `Jet.GrammarUndefinedRule` | A reference names no rule, or an `import` names no grammar in the module | the reference |
| `Jet.GrammarStartRule` | The binding name is no rule's PascalCase name | the binding name |
| `Jet.GrammarLeftRecursive` | Left recursion that is not the operator form; the message gives the cycle `expr -> term -> expr` | the first rule in the cycle |
| `Jet.GrammarAmbiguous` | Two alternatives, or continue/stop of a repeat or optional, share a lookahead of k ≤ 3 tokens; the message gives both and a sample input | both alternatives |
| `Jet.GrammarTooComplex` | The lookahead search at one decision passes its cap (§6) | the decision |
| `Jet.GrammarEmptyRepeat` | A repeat whose item can match nothing | the repeat |
| `Jet.GrammarTokenClash` | Two token classes match the same text at one decision | both tokens |
| `Jet.GrammarFieldShape` | A mix of labeled and unlabeled alternatives, a label of two types, a rule referenced twice without labels, or a label clashing with a rule name (ANTLR error 124) | the alternative |
| `Jet.GrammarUnprintable` | Two adjacent tokens lex as one and no `skip` token separates them, or an unlabeled token has no literal text | the pair |
| `Jet.CheckedTextHoleSlot` | `accepts` refused a hole's type for its slot, or a hole sits where the grammar has no `HOLE` | the hole |
| `Jet.CheckedTextRejected` (E2712, kept) | A head body breaks its grammar; reported at its exact `line:column`, one error per resync point | the spot |
| `Jet.InvalidRegexLiteral` (E0152) | Retired into E2712, with the same note as E0155's retirement | — |

The first draft listed `Jet.GrammarBackReference`; it is gone with
back-references. Run-time `parse` returns `GrammarError`. Its `line` and
`column` (1-based, in code points) are computed only when an error is built.

## 6. The analysis: ambiguity and left recursion

The parsing class is **SLL(k)**, also called strong LL(k), with k ≤ 3.
Decisions use FOLLOW sets of the whole grammar, not the calling context.
This is weaker than full LL(k) for k > 1: a few LL(k) grammars are refused,
and the fix is factoring (R-H3). There is no ordered choice, no backtracking
and no memo table. Every conflict is a build error.

The analysis runs in `core.grammar`, in this order:

1. **Resolve.** Names, casing, labels, the start rule and `import`.
2. **Nullable.** Computed by a fixed point.
3. **Left recursion.** Each directly left-recursive rule is split into
   primaries and operator alternatives (§3.1). Any remaining cycle in the
   "can begin without consuming input" graph is reported with its path.
4. **Empty repeats.**
5. **LL(1).** FIRST_1 and FOLLOW_1 over contextual alphabets. Most decisions
   end here.
6. **Lazy deepening, only at LL(1) conflicts** (R-H3). The analysis explores
   the token sequences of the conflicting branches to depth 2, then 3. Each
   decision has a cap of 4,096 explored sequences. Passing the cap gives
   `Jet.GrammarTooComplex` instead of the |T|^k growth of eager FIRST_k
   sets. The shortest sample the branches share is shown as text.
7. **Tokens.** Contextual viability, the literal-wins rule (§3.2), longest
   match inside a token, and token clashes.
8. **Precedence.** Levels, associativity and the middle-operand rule. The
   same literal may not be the infix operator of two alternatives.
9. **Printer.** Separators, canonical token text and the grouping
   alternative.
10. **Shapes.** Types and fields.

LL(k) conflict is a strict superset of real ambiguity, which is undecidable.
So the message says "the parser cannot choose between these within 3
tokens", and the fix offers factoring or the operator form.

**Cost.**

- Steps 1–5 and 7–10 are linear in grammar size.
- Step 6 is bounded by the cap for each conflicting decision.
- The analysis runs in the compile-time child, so it must fit the step limit
  of 10,000,000 per compile-time call (E0952) without `#StepLimit`. G-A16
  records the steps for a 400-rule grammar, and the real SQL grammar proves
  it in G-D11.
- Core's grammars are analyzed once, when the Core library is checked, and
  read from the Core bodies record afterward (§2.2).
- A user grammar costs one batch child run per cold compile. G-P1 measures
  it.

## 7. Performance plan

**Technique: direct-coded SLL(k) recursive descent with contextual,
on-demand lexing.**

- **One function per rule.** A decision is a `match` on the next byte class
  or token kind, which lowers to a jump table.
- **Byte-class tables** (256 entries) are computed at build time.
- **Keywords:** a word-shaped scan, then one lookup in a perfect hash built at
  build time, as SQLite's `keywordhash.h` does. This replaces the first
  draft's compare of length then word, which is linear in the number of
  candidates (R-M6).
- **Greedy class runs** (identifiers, digits, spaces, string bodies) use a
  byte loop over the class table. Scanning 8 bytes at a time (SWAR) is a
  later generator improvement, and only if a pair needs it. Nothing in Core
  does SWAR today [UNVERIFIED that codegen supports it; §16].
- **Zero-copy tokens.** Tokens are spans; a `String` is made only for a field.
- **Precedence climbing** for operator rules. No per-token heap allocation in
  `parse_into`; tree mode allocates exactly the generated values.
- **Sink mode** drives existing builders without a tree: the SQLite arena
  builder, the JSON `Dyn` builder and the XML engine.

**Run-time gate.** `Tools/Perf/GrammarPair.jet` follows `ZeroCostPair.jet`:

- two arms (hand-written and generated);
- one corpus;
- `--release` at -O1 and -O2;
- alternating runs through the gauntlet launcher;
- the shared paired-bootstrap interval on wall time, cycles and peak memory.

It exits 0 only when no metric is a loss. Each pair is a row of a new
`grammar_pairs` list in `Tools/Gauntlet/measurement-manifest.json`. The
command is `jet Tools/Perf/GrammarPair.jet --pair <name> [--output FILE]`. It
is a timed measurement: the orchestrator runs it alone, on the reference
machine.

**Compile-time gate (R-C1, a required cell).**
`JET=<candidate jet> jet Tools/Perf/CompileThroughput/Throughput.jet --against <seed jet> --modes check,O0`.
Its corpus is the compiler plus all of Core, every compile cold and uncached.
It fails when the median CPU or the instruction count rises by more than 10%.

- It runs once when G-B8 lands, as G-P1.
- It runs again before the hand SQL parser is deleted, as G-P2.

A hand-written parser is deleted only in the landing whose pair passes. A
losing pair is fixed in the generator, never by a special case.

## 8. Worked examples

### 8.1 INI

```jet
use core.grammar.[Grammar]

Ini: Grammar{"""
    ini     : lines+=line (EOL lines+=line)* ;
    line    : section | entry | blank ;
    section : '[' name=KEY ']' ;
    entry   : name=KEY SEP value? ;
    blank   : ;
    value   : text=QUOTED   # Quoted
            | text=BARE     # Bare
            ;
    KEY     : [A-Za-z0-9_.\-]+ ;
    SEP     : '=' | ':' ;
    QUOTED  : '"' ([^"\\\r\n] | '\\' .)* '"' | '\'' [^'\r\n]* '\'' ;
    BARE    : [^ \t\r\n;#"'] [^\r\n;#]* ;
    EOL     : '\r'? '\n' ;
    WS      : [ \t]+ -> skip ;
    COMMENT : [;#] [^\r\n]* -> skip ;
"""}
```

Generated (shown as the declarations sema checks):

```jet
Ini: struct { lines: [Line] }
Line: enum { Section(Section), Entry(Entry), Blank(Blank) }
Section: struct { name: String }
Entry: struct { name: String, value: Value? }
Blank: struct {}
Value: enum { Quoted{text: String}, Bare{text: String} }
```

- `line` can match nothing (`blank`), but `EOL` cannot, so the repeat
  `(EOL lines+=line)*` is not empty.
- Contextual lexing keeps `KEY` and `BARE` apart. At a line start, `BARE` is
  not viable; after `SEP`, `KEY` is not.
- Use: `doc: Ini.parse(source) ?? panic("{err}")`.
- An error: `Ini.parse("[server\nport = 1")` gives `1:8: expected "]", found
  end of line`.
- `doc.print()?` writes `[server]` and `port = 1`. `SEP` prints `=`, its first
  literal.

### 8.2 An expression language with precedence

```jet
Calc: Grammar{"""
    calc : body=expr ;
    expr : operand=expr '(' (args+=expr (',' args+=expr)*)? ')'   # Call
         | <assoc=right> left=expr '^' right=expr                 # Pow
         | '-' operand=expr                                       # Neg
         | left=expr op=('*' | '/' | '%') right=expr              # Mul
         | left=expr op=('+' | '-') right=expr                    # Add
         | value=NUM                                              # Num
         | name=IDENT                                             # Var
         | '(' inner=expr ')'                                     # Group
         ;
    NUM   : [0-9]+ ('.' [0-9]+)? ;
    IDENT : [A-Za-z_] [A-Za-z0-9_]* ;
    WS    : [ \t\r\n]+ -> skip ;
"""}
```

Generated:

```jet
Calc: struct { body: Expr }
Expr: enum {
    Call{operand: Expr, args: [Expr]},
    Pow{left: Expr, right: Expr},
    Neg{operand: Expr},
    Mul{left: Expr, op: String, right: Expr},
    Add{left: Expr, op: String, right: Expr},
    Num{value: String},
    Var{name: String},
    Group{inner: Expr},
}
```

- `1 + 2 * 3` is `Add{1, "+", Mul{2, "*", 3}}`; `2 ^ 3 ^ 2` is
  `Pow{2, Pow{3, 2}}`; `-2 ^ 2` is `Neg{Pow{2, 2}}`; `2 ^ -1` is
  `Pow{2, Neg{1}}`.
- `(` at the start of an operand is `Group`. After an operand it is `Call`.
- Indirect left recursion fails. For example, `expr : term '+' NUM | NUM ;`
  with `term : expr '*' NUM ;` gives `Jet.GrammarLeftRecursive`: "rule expr is
  left-recursive (expr -> term -> expr)". The fix offers the operator form.
- A hand-built `Mul{Add{1, "+", 2}, "*", 3}` prints `(1 + 2) * 3` through the
  `Group` alternative.
- A number stays `String`. `Float.parse(value)` converts it in code. The
  grammar has no typed number tokens, and none are needed.

### 8.3 A log line

```jet
LogLine: Grammar{"""
    log_line : at=STAMP level target=PATH ':' message=REST ;
    level    : 'TRACE' # Trace | 'DEBUG' # Debug | 'INFO' # Info | 'WARN' # Warn | 'ERROR' # Error ;
    STAMP    : [0-9]+ '-' [0-9]+ '-' [0-9]+ 'T' [0-9]+ ':' [0-9]+ ':' [0-9]+ ('.' [0-9]+)? ('Z' | [+\-] [0-9]+ ':' [0-9]+) ;
    PATH     : [a-z_] [a-z0-9_.]* ;
    REST     : [^\n]* ;
    WS       : ' '+ -> skip ;
"""}
```

This gives `LogLine{at: String, level: Level, target: String, message: String}`
and `Level: enum { Trace, Debug, Info, Warn, Error }`.

- `2026-11-25T10:15:00Z INFO  http.server: GET /index 200` parses.
- `… NOTE x: y` fails with `1:22: expected one of "TRACE", "DEBUG", "INFO",
  "WARN", "ERROR", found "NOTE"`.
- `INFO` inside `message` stays text, because no keyword is viable at `REST`.

### 8.4 The SQL head: holes typed by position

The grammar lives in `Core/DB/Syntax/SQL.jet` as `priv Script: Grammar{…}`.
Only the hole-bearing rules are shown:

```text
script        : statements+=statement (';' statements+=statement?)* ;
name          : text=ID # Bare | text=QUOTED_ID # Quoted | word=fallback_keyword # Keyword
              | HOLE<slot=Name> # Hole ;
column_ref    : (schema=plain_name '.')? (table=plain_name '.')? column=plain_name ;
plain_name    : text=ID # Bare | text=QUOTED_ID # Quoted | word=fallback_keyword # Keyword ;
primary       : literal | column_ref | HOLE<slot=Operand> # Hole | … ;
qualified_table : (schema=name '.')? table=name ;
```

The first draft gave two slots, `Value` and `Name`. Both were viable at
every expression position, which was an LL conflict, and a column hole could
have been bound as a value (R-C4). The slots are now:

- **`Operand`**, wherever an expression operand may stand. The hole's type
  decides:
  - `SQLName` is spliced as a quoted column reference;
  - `Int`, `Float`, `Bool`, `[U8]`, `String`, `DBValue` and the other scalar
    types are bound as a `DBValue`;
  - a list or other collection is refused by `accepts` (R-M8). Today it binds
    its display text `"[1, 2]"` (`DB.jet:341`). Fix: build `IN` with
    `SQL.raw` under `#Unsafe`, or bind a JSON value with `json_each`.
- **`Name`**, wherever only an identifier may stand: table, schema, alias, an
  `INSERT` column list, or an index name. It accepts only `SQLName`, a new
  checked identifier type in `core.db` that refuses NUL and empty text. The
  value is double-quoted with `"` doubled, never bound.

```jet
recent: fn(db: Conn, table: SQLName, since: Int) -[DB.Read]> [[String:DBValue]] DBError! {
    db.query(SQL{"SELECT id, title FROM {table} WHERE created > {since} ORDER BY created"})
}
```

The build-time shape is `SELECT id, title FROM ¤ WHERE created > ? ORDER BY
created`, with slots `[Name, Operand]`. `build` splices the quoted name and
binds `since`.

| Body | Result |
|---|---|
| `SQL{"SELEC id FROM t"}` | E2712 at `1:1`: expected a statement keyword, found `SELEC` |
| `SQL{"SELECT * FROM {name}"}` with `name: String` | `Jet.CheckedTextHoleSlot` at the hole: SQL expects a table name here, and a `String` would be bound as a value. Fix: make it `SQLName{…}` |
| `SQL{"SELECT * FROM t WHERE a = '{x}'"}` | `Jet.CheckedTextHoleSlot`: a hole cannot sit inside an SQL string literal. Fix: write `a = {x}` |
| `SQL{"SELECT * FROM t WHERE id IN ({ids})"}` with `ids: [Int]` | `Jet.CheckedTextHoleSlot`: a list cannot be one bound value |
| `SQL{"CREATE TABLE select (a)"}` | E2712: `select` is a reserved word here; quote it as `"select"` |
| `SQL{"SELECT 1; DROP TABLE t"}` | Accepted by the grammar (two statements). The one-statement rule of policy scoping reads `script.statements.len()` |

### 8.5 The HTML head: escaping chosen by position

The grammar lives in `Core/Text/Syntax/HTML.jet`. It is strict and
well-formed:

- void elements need no close;
- raw-text elements (`script`, `style`) and comments are tokens;
- the head's check compares open and close tag names ASCII
  case-insensitively over the tree.

```text
element    : '<' tag=TAG_NAME attrs+=attr* ('/>' | '>' children+=node* '</' close=TAG_NAME '>') ;
attr       : name=ATTR_NAME ('=' value=attr_value)? ;
attr_value : '"' parts+=dq_part* '"' # Double | '\'' parts+=sq_part* '\'' # Single
           | text=UNQUOTED # Unquoted | HOLE<slot=Attr> # Hole ;
dq_part    : text=DQ_TEXT # Text | HOLE<slot=Attr> # Hole ;
node       : element # Element | text=TEXT # Text | HOLE<slot=Text> # Hole | COMMENT # Comment | … ;
```

The grammar gives the syntactic slot. The head's `check` refines `Attr` by
the attribute's name and quoting (R-C3):

| Slot | When | Escaping |
|---|---|---|
| `Text` | element content, including RCDATA (`title`, `textarea`) | `&`, `<`, `>` |
| `QuotedAttr` | a `"…"` or `'…'` value of an ordinary attribute | `&`, `<`, `>`, `"`, `'` |
| `UnquotedAttr` | an unquoted value | the quoted set, plus whitespace, `=` and backtick; an empty value becomes `""` |
| `URLAttr` | the URL attributes of Go's `html/template` (`attrTypeMap` in `src/html/template/attr.go`, read 2026-11-25): `action`, `archive`, `background`, `cite`, `classid`, `codebase`, `data`, `formaction`, `href`, `icon`, `longdesc`, `manifest`, `poster`, `profile`, `src`, `usemap`, `xmlns`. Go's name rules apply: a `data-` prefix is stripped first; a `prefix:` is stripped (`xlink:href` → `href`), except that `xmlns:*` is URL; any other name containing `src`, `uri` or `url` is URL | see below |
| `Refused` (B8) | Go's script, CSS, HTML and srcset types: any name that starts with `on` after the stripping above (so `data-onclick` too), `style`, `srcdoc`, `srcset` | a hole is `Jet.CheckedTextHoleSlot` |

For `URLAttr`:

- **A hole at the start of the value**, whether `String` or `URL`, is
  scheme-checked at run time by Go's `isSafeURL` rule. If the text has a `:`
  with no `/` before it, the part before the `:` must be `http`, `https` or
  `mailto`, compared ASCII case-insensitively. `\tjavascript:` and
  `java\nscript:` therefore fail too, with no stripping. A failing value
  becomes `#ZgotmplZ`-style `about:invalid#jet`. Go exempts its
  `template.URL` type from this filter. Jet does not exempt `URL`, because a
  parsed `URL` can hold `javascript:` and asserts nothing about safety
  (R-C3).
- **A hole after a fixed prefix** (`href="/user/{id}"`) is percent-encoded
  for its URL position, then attribute-escaped.

An `HTML`-typed hole composes without escaping, and only in the `Text` slot.
Under B8 (decided as A, §14), a hole in a `Refused` attribute is an error.

| Body | Result |
|---|---|
| `<div><span></div>` | E2712 at `1:12`: expected `</span>`, found `</div>` |
| `<{tag}>` | `Jet.CheckedTextHoleSlot`: a hole cannot be a tag name |
| `<script>{x}</script>` | `Jet.CheckedTextHoleSlot`: a hole cannot sit inside a raw-text element |
| `<a onclick="go({x})">` | `Jet.CheckedTextHoleSlot` (B8): a hole inside a script attribute would run as code |

### 8.6 The Sh head: one hole, one argv item

The grammar lives in `Core/Process/Syntax/Sh.jet`. Spaces are explicit tokens,
because the joins between pieces are part of the meaning:

```text
command : SP? words+=word (SP words+=word)* SP? ;
word    : pieces+=piece+ ;
piece   : text=BARE # Bare | text=SINGLE # Single | '"' parts+=dpart* '"' # Double
        | HOLE<slot=Arg> # Hole ;
dpart   : text=DTEXT # Text | HOLE<slot=Arg> # Hole ;
BARE    : ([^ \t\r\n'"\\|&;<>()$`] | '\\' [^\r\n])+ ;
SINGLE  : '\'' [^']* '\'' ;
DTEXT   : ([^"\\$`] | '\\' .)+ ;
SP      : ([ \t\r\n] | '\\' '\r'? '\n')+ ;
```

- No token contains an unquoted `| & ; < > ( ) $` or backtick, so each is a
  build error (B5).
- Under B7, `SP` drops the bare `\r` and `\n`, so a newline that is not
  continued with `\` is an error, as a shell would treat it as a command
  separator. `$` and backtick inside double quotes are refused too (R-H6).
- `Sh{"git commit -m {message} --author={who} -- *.jet"}` gives argv
  `["git", "commit", "-m", message, "--author=" + who, "--", "*.jet"]`.
  `message` is one item, even when empty or full of spaces. `who` is glued
  into one word, and `*.jet` is passed literally.
- The `Shape` is the argv plan of literal words and glue points, so nothing is
  split again at run time. This fixes today's `a{x}b` → three items.
- Under B7, a hole in the first word (the program) accepts only `Path`
  (R-H6).

| Body | Result |
|---|---|
| `Sh{"ls | wc"}` | E2712 at `1:4`: `|` is a shell pipe, and `Sh` runs one program without a shell. Fix: quote it as `'|'`, or run two commands |
| `Sh{"echo 'unclosed"}` | E2712 at `1:6`: unclosed quote |
| A `[String]` hole | `Jet.CheckedTextHoleSlot`: a list would need splitting. Fix: build the argument list directly |
| `Sh{"{tool} --help"}` with `tool: String` | Under B7, `Jet.CheckedTextHoleSlot`: a `String` cannot choose the program. Fix: pass a `Path` |

## 9. Per-head plan

### 9.1 SQL (criterion 3)

- **One grammar** in `Core/DB/Syntax/SQL.jet`, written from SQLite 3.45.0's
  `src/parse.y`. That is the source of the tables the Lemon port carries
  (`ParseTables.jet:1-6`; 405 rules). `parse.y` is not in the repository; the
  item author reads the 3.45.0 release source. The `fallback_keyword` rule
  lists exactly the tokens with a non-zero entry in the port's `yyFallback`
  table (`ParseTables.jet:6013`), so no keyword list is copied by hand.
- **The head:**
  - `Slot = SQLSlot{Operand, Name}`;
  - `Hole = SQLHole{Value(DBValue), Name(SQLName)}`;
  - `Error = [GrammarError]`, with `at` spots;
  - reserved words are rejected as in §3.2.
- **The engine** is fed through `parse_into` with a sink that drives the
  existing arena `SQLBuilder`. Lemon reduce actions become `close(rule)`
  handlers. `Parse.jet`, `ParseTables.jet` and the hand tokenizer go once the
  `sql_engine` pair and G-P2 pass.
- **The analytics subset** (`Core/DB/SQL.jet`) becomes a shape check over the
  tree, and its parser is deleted.
- **Policy scoping** becomes queries on the tree. Every accept and refuse of
  `Tests/Cases/DBPolicy/` is kept, and a security check built on byte scans
  goes away.
- **Typed blocks** `#SQL(Row) { … }` keep their row check.

### 9.2 HTML (criterion 6)

- One grammar, as in §8.5. Delete `html_state_after`, the
  `HTML_TEXT`..`HTML_SINGLE_VALUE` constants and `html_value_hole`
  (`HTML.jet:147-205`).
- spec.md:93-99 is reworded to name the slots and the URL rule. Line 97's `^`
  becomes `&`.

### 9.3 Sh (criterion 7)

- As in §8.6. Delete `sh_split_into`.
- `Tests/Cases/CoreLibPartsPlatform/core_process_sh_typed_text_keeps_each_hole_one_argv_item.case`
  keeps its expected output.

### 9.4 URL (criterion 8)

- An RFC 3986 URI-reference grammar in `Core/Net/Syntax/URL.jet`.
- **Slots:**
  - `Host`, which accepts a `Host` or `IP` value, or a `String` checked at run
    time against reg-name. A failure stops at the literal, as `build` failures
    do today;
  - `Port`, which accepts only `Int`;
  - `Userinfo`, `Segment`, `QueryKey`, `QueryValue` and `Fragment`, each
    percent-encoded for its own reserved set.

  A hole in the scheme is refused (R-M7).
- Delete `url_hole_marker` and `url_literal_reason`.
- Run-time `URL.parse` uses the same grammar. `parse_recipe`, `parse_qsl`
  and `parse_port` become rules.

### 9.5 DateTime (criterion 8)

- An RFC 3339 grammar in `Core/Time/Syntax/RFC3339.jet`. Range checks (month
  1..12, hour 0..23, day of month, leap second) are code over the tree,
  because the notation has no typed number ranges.
- The head's `Shape` is the `DateTime`, so the run-time re-parse in `build`
  goes away. A `DateTime` must be representable as a compile-time value
  [UNVERIFIED; §16].
- `parse_rfc3339`, `civil_parse_date`, `parse_time`, `civil_parse_offset` and
  `parse_iso_week_date` become rules. `parse_zoned` keeps its zone lookup in
  code.

### 9.6 Path (criterion 8)

- A one-rule grammar: parts with `HOLE<slot=Component>`, plus the NUL refusal
  as a class.
- `build` keeps `path_component_encode`, as `Files.jet:73-79` does today.

### 9.7 Regex (criterion 8)

- A regex syntax grammar in `Core/Regex/Syntax/Regex.jet` feeds the existing
  compiler through a sink. It replaces the 14 `parse_*` methods of
  `Core/Regex/Regex.jet`, subject to the `regex_syntax` pair (R-H7 path fix).
- `Regex` implements `CheckedText`. Its `Shape` is the compiled program, and
  `accepts` refuses every hole.
- Delete `sema_expression_typed_literal_regex` and `sema_expression_is_regex_head`
  (`Constructors.jet:2340-2393`) and the `boundary_regex` import. The compiler
  then stops running `core.regex` in-process. Retire E0152 into E2712.

## 10. Core parser survey and classification

Method: every function whose name matches
`(^|_)(un)?(parse|parses|parsed|lex|lexeme|tokenize|tokens)(_|$)` in `Core/`.
The first draft counted 317 functions in 77 files at `4b48f2bc485`. The
review recounted at `e1d1b579653`, counting declarations `name: fn` outside
`.jet/` folders, and found 315 in 77. The brief said about 246 in 104; its
pattern differs.

| Format / file(s) | Parser functions | Class | Grammar / reason | Pair |
|---|---|---|---|---|
| SQL engine: `DB/Engine/Parse.jet`, `ParseTables.jet`, `Tokenize.jet` | Lemon driver and tables | **Replace** (sink) | `SQL` | `sql_engine` |
| SQL analytics: `DB/SQL.jet` | `parse_sql_query`, `sql_tokens` | **Replace** | `SQL` + shape check | covered by `sql_engine` |
| SQL policy: `DB/DB.jet:404-724, 3419-3460` | `sql_tokens` and 20 helpers | **Replace** | `SQL` tree queries | `sql_policy` |
| JSON: `Encoding/JSON.jet`, `JSONL.jet`, `Web/Router.jet` `parse_json_query` | value and string readers | **Replace** (sink to `Dyn`) | `Json` | `json` |
| JSON5 / json1: `DB/Engine/Func.jet` `json_parse_*` | a duplicate JSON reader | **Replace** | `Json5`, which imports `Json` and overrides rules | `json5` |
| TOML: `Encoding/TOML.jet` | | **Replace** | `Toml`; dates through RFC 3339 | `toml` |
| INI: `Encoding/Ini.jet` | `parse`, `strip_comment`, `unquote`, `split_lines`, `ini_value` | **Replace** | `Ini` | `ini` |
| XML: `Encoding/XMLEngine.jet` (3,981 lines), `XMLKernel.jet`, `XML.jet` | `parse_document*`, `parse_name`, `parse_parts` | **Replace** (sink) | `Xml`; close-tag match, entities and the DTD subset in the sink | `xml` |
| URL: `Net/URL.jet` | `parse_recipe`, `parse_qsl`, `parse_port` | **Replace** | `URL` | `url` |
| IP: `Net/IP.jet` + `Net/Net.jet` duplicates | | **Replace, deleting the duplicate** | `Ip` | `ip` |
| MIME: `Net/MIME.jet` `parse` | | **Replace** | `Mime` | `mime` |
| HTTP head: `HTTP/HTTP.jet`, `Client.jet`, `Server.jet` `route_parse` | request and status lines, headers, cookies, routes | **Replace** (sink; bodies stay code) | `HttpHead`, `Route` | `http_head` |
| Time: `Time/Time.jet` | date, ISO week, time, RFC 3339, offset | **Replace** | `RFC3339`, `IsoWeek` | `rfc3339` |
| Cron, app sync: `App/App.jet` | `cron_parse`, `sync_document_parse`, `sync_count_parse` | **Replace** | `Cron`, `SyncDocument` | `cron`, `sync` |
| Log filter, UUID, PEM armor, policy join | `parse_filter`, UUID `parse`, armor lines, `table_parse_policy_join` | **Replace** | `LogFilter`, `Uuid`, `Pem`, `PolicyJoin` | one each |
| Xkb, service records | `lex`, `parse_keymap`; `authority_parse_*`, `delivery_parse_state`, `state_parent_lex` | **Replace** | `Xkb`, `ServiceRecord` | one each |
| Data format names: `Data/Data.jet` `parse_format` | keyword lookup | **Keep** | a keyword table, not a grammar | — |
| Regex syntax: `Regex/Regex.jet` (14 `parse_*`) | | **Replace** (sink) | `Regex` (§9.7) | `regex_syntax` |
| Numbers: `Text/Parse.jet`, `Math.jet` `decimal_parse` | | **Keep as the one number primitive** | a primitive that code over trees calls | — |
| Duplicate integer parsers (`Log`, `Crypto`, `Service`, `Browser`, `HTTP/Client`, `Encoding/Stream`, Term, Net, URL, Archive) | | **Delete, calling `core.text.parse`** | duplicates of the primitive | — |
| YAML | | **Keep** | indentation- and context-sensitive | — |
| CSV | | **Keep** | delimiter, quote and header chosen at run time | — |
| Args | | **Keep** | input is a list, not text | — |
| Path components: `Files/Runtime/KernelPathComponents.jet` | `parse_prefix*` | **Keep** | a byte split, already optimal | — |
| Stream: `Encoding/Stream.jet` `parse_value` | incremental pull reader | **Keep** | streaming, resumable input; recorded as a limit (§12) | — |
| Binary formats (SSL, DER/X509/PKCS, Parquet, Archive, CBOR, Btree/Record, Zstd, `parse_atlas`) | | **Keep** | binary layouts | — |
| `Compiler/Compiler.jet` `lex`/`parse` | Jet's own parser | **Keep** | one Jet parser | — |
| Engine plumbing (`nested_parse`, `take_parse`, `vtab_*_parse`, `parse_schema`) | callers into the SQL parser | **Re-point** to `parse_into` | — | — |
| False positives (Auth `tokens_*`, `number_lexeme`, `app_session_tokens`, `parse_locked`) | | — | — | — |

## 11. Cutover list (every copy this deletes or changes)

Each change lands with its replacement, in the same landing, with no shim
(Greenfield cutover). A parser deletion also needs its pair to pass.

1. **CheckedText amendment (one landing: G-B1 to G-B3g)** (R-C2).
   - Compiler:
     - `Compiler/Prelude/Core/prelude.jet`;
     - `OwnedTraitContracts.jet:432-439`;
     - `Constructors.jet` (literal, `from`, `raw` and run-time paths, E0907);
     - `Effects/Declarations.jet:367`;
     - `ComptimePrecheck.jet:1154`;
     - `TypeRelations.jet:2431-2452`;
     - `JetSemIndex/…/Symbols.jet:295`;
     - `GateLedger.jet:90-104`;
     - `Pipeline.jet` (the `jet_driver_checked_text_reason` sniffing).
   - Core: `DB.jet`, `HTML.jet`, `Process.jet`, `URL.jet`, `Time.jet` and
     `Files.jet`.
   - Examples:
     - every example under `Examples/Text/CheckedText/` that implements or
       calls the trait: `CheckedTextBoundHoles.jet`,
       `CheckedTextNeverRejects.jet`, `CheckedTextRuntimeHole.jet`,
       `CheckedTextRuntimeHoleStop.jet`, `CheckedTextType.jet`,
       `CheckedTextSQL.jet` (its `.out`), and
       `CheckedText/types/module.jet` (four impls);
     - `Examples/Types/Traits/UserCheckedTextShape.jet`;
     - `Examples/Modules/Packages/CypherHead/Cypher/cypher.jet`.
   - Cases:
     - `Tests/Cases/CheckedText/*.fixture` (10);
     - `Tests/Cases/CheckedTextCrossings/` (both fixtures);
     - `Tests/Cases/ComptimeCLIParity/` (`tooling_checked_text_type`);
     - `Tests/Cases/Fmt/FmtPreservesCheckedTextType`;
     - `Tests/Cases/JetDoc/CheckedTextContractIsRenderedInGeneratedDocs.fixture`;
     - `Tests/Cases/LSP/lsp_checked_text_contract_survives_editor_navigation.case`
       (embedded source) and its fixture;
     - `Tests/Cases/LanguageFeatures/ImplMethodInheritsTraitEffectRowOnEveryTier.fixture`;
     - `Tests/Cases/Reflect/CheckedTextContractIsReflectedOnTheNominalType.fixture`;
     - `Tests/Cases/Semindex/CheckedTextContractIsOneToolingFact.fixture`.
   - UI: `Tests/UI/checked_text_build_contract.jet`,
     `checked_text_build_rejects_skeleton.jet`, `checked_text_check_reason.jet`,
     `checked_text_check_without_row_refuses_io.jet`,
     `checked_text_raw_requires_unsafe.jet`, `sink_needs_checked_text.jet`,
     `typed_literal_text_body_needs_checked_text.stderr` and
     `Tests/UI/PackageHead/cypher/cypher.jet`.
   - Spec: `Docs/Spec/spec.md:88-116`.
2. **Sh:** `sh_split_into`.
3. **HTML:** `html_state_after`, the `HTML_*` state constants and
   `html_value_hole`.
4. **URL:** `url_hole_marker`, `url_literal_reason`, `parse_recipe`,
   `parse_qsl` and `parse_port`.
5. **DateTime:** the re-parse in `build`, and the hand date and time
   parsers.
6. **Regex:** `sema_expression_typed_literal_regex`,
   `sema_expression_is_regex_head`, the `boundary_regex` import and E0152;
   the 14 hand `parse_*` once the pair passes.
7. **SQL:** the `Core/DB/SQL.jet` parser and tokenizer; the `DB.jet`
   `sql_tokens` family; `Engine/Parse.jet`, `ParseTables.jet` and the
   `Tokenize.jet` hand lexer.
8. **JSON, Net and the other text formats**, as listed in §10.
9. **Duplicate integer parsers.**
10. **Spec, reference, registry and editors:**
    - `Docs/Spec/spec.md` (the checked-text section and a new "Grammar
      literals" section);
    - `Docs/Spec/syntax-decisions.md` (the P31 approvals and the amended
      D-TEXTHEAD-HOLES1, D-TEXTHEAD-TYPE1 and D-META-LADDER1 text);
    - `Docs/Spec/Reference/core-library.md`;
    - `Compiler/Prelude/Core.rows` and `Core/package.jet` (`core.grammar`);
    - `Spellings.jet` (a `Grammar` declaring-head row);
    - `Compiler/Prelude/Diagnostics.jet` with its regenerated copies;
    - the editor grammars, through `jet self devtools grammars`.

## 12. Edge cases

1. **Braces.** A hole in a `Grammar` literal is refused. Literal braces are
   `'{{'`/`'}}'`.
2. **Trivia** is dropped by `print`. Tools that need it use `parse_into`,
   whose sink sees `skip` spans as `token(SKIP, …)`.
3. **Very deep nesting.** One Core-wide bound, `GRAMMAR_MAX_DEPTH` in
   `Core/Grammar/Runtime.jet`, is set at G-A15 to at least the largest depth
   limit of the parsers it replaces. Deeper input gives a `GrammarError`,
   never a stack overflow. A smaller limit for one format (JSON) is enforced
   by its sink. This replaces the first draft's `depth =` option, whose
   braces could not be written.
4. **Huge inputs.** Offsets are `Int`. Line and column are computed only on
   error.
5. **Encoding.** Input is UTF-8 `String`. Columns count code points.
   `parse_into` over bytes is out of scope.
6. **CRLF.** Written as `'\r'? '\n'` in the grammar. Columns restart after
   `\n`.
7. **Case-insensitive keywords** print in the grammar's spelling.
8. **Empty input.** `R.parse("")` succeeds only when the start rule is
   nullable.
9. **Trailing input.** `parse` requires `EOF`. A prefix parse uses
   `parse_into` and reads the returned offset.
10. **Resync** never invents input. After an error the result is `Err`.
11. **Grammar reuse.** `import` names a grammar of the same module only.
12. **Streaming and indentation-sensitive input** (Stream, YAML) are not
    served. These are recorded limits.
13. **Name clashes** are the ordinary duplicate-declaration error.
14. **Order** matters only among the operator alternatives of a left-recursive
    rule, where it is the precedence (§3.1). Everywhere else alternatives are
    unordered.
15. **Printing hand-built values** is checked (§4): a token field that breaks
    its token rule returns a `GrammarError`.

## 13. Comparison with peers

| Peer | Notation | Parsing class | Left recursion | Ambiguity | Types from the grammar | Source |
|---|---|---|---|---|---|---|
| ANTLR 4 | `rule : … ;`, `'lit'`, `label=`, `+=`, `# Alt` | ALL(*): SLL first, then full LL | Direct, by rewrite into precedence climbing | Resolved at run time by alternative order | A generated context class per rule and labeled alternative | parser-rules.md, left-recursion.md (§3); <https://www.antlr.org/papers/allstar-techreport.pdf> |
| Bison / yacc | `result: … ;`, `%left`/`%right` | LALR(1), GLR option | Allowed | Conflicts reported; default resolution shift over reduce | None (semantic values by hand) | Bison manual 3.3.1 |
| lark (Python) | `rule: …`, `"x"i`, `-> alias`, `%ignore` | Earley, LALR(1) or CYK; contextual lexer | Allowed | Earley keeps all parses; LALR reports conflicts | No; a `Tree` plus transformers | grammar reference (§3) |
| pest (Rust) | `rule = { … }`, `!`/`&` predicates | PEG, ordered choice; Pratt helper | A build error naming the cycle | Hidden by ordered choice | No; a generic `Pair` tree | <https://pest.rs/book/grammars/syntax.html>; `meta/src/validator.rs` |
| Raku grammars | `token`/`rule`/`regex` | Backtracking regex with ratcheting | Author's job | Hidden by ordered/longest-token semantics | Match objects plus action classes | <https://docs.raku.org/language/grammars> |
| tree-sitter | JavaScript DSL | GLR with declared `conflicts` | Allowed | Build error unless declared | A generic concrete syntax tree | <https://tree-sitter.github.io/tree-sitter/creating-parsers/3-writing-the-grammar.html> |
| nom (Rust) | combinators in code | Hand-composed, zero-copy, streaming | Author's job | None | The author's own types | <https://docs.rs/nom/latest/nom/> |
| Zig comptime | code | Arbitrary compile-time code; `std.fmt` parses format strings at compile time | n/a | n/a | Comptime can return types | <https://ziglang.org/documentation/0.14.0/#Case-Study-print-in-Zig> |
| Jai | — | `#insert` of generated code | n/a | n/a | by text insertion | [UNVERIFIED: no official public docs] |
| **Jet (this design)** | the ANTLR 4/Bison form, plus `HOLE<slot=…>` | SLL(k ≤ 3), direct-coded, contextual lexing; ANTLR-form precedence | **Operator form allowed; any other form is a build error naming the cycle** | **A build error with both alternatives and a sample**, never resolved by order | **Nominal Jet types per rule, plus `parse`/`print`/sink, checked at build time and used for typed holes** | — |

What Jet takes from each:

- **ANTLR:** the notation, labels, typed nodes and operator form, without its
  silent resolution by order.
- **Bison/SQLite:** the keyword priority and explicit fallback.
- **Lark:** the contextual lexer and `"x"i`.
- **pest:** the left-recursion message.
- **tree-sitter:** strict conflict reports, with no escape by declaring
  conflicts.
- **nom:** zero-copy spans.

Unlike Zig and Jai, Jet does not let arbitrary compile-time code make types.
One reviewed Core grammar compiler makes plans, and one thin compiler builder
makes items from them.

## 14. Decisions

**Owner-approved as part of P31 (2026-11-25); no ballot needed:**

- the declaring form `Name: Grammar{"""…"""}` (first draft's B1). This
  includes the PascalCase binding, its casing exemption, the `Spellings.jet`
  row, the supersession of D-TEXTHEAD-TYPE1's rejection for this head, and
  this sanctioned route to types from text under D-META-LADDER1;
- the standard notation of §3 (first draft's B2, now rewritten to the
  standard);
- the `CheckedText` change of §2.3 (B3);
- the Core surface names (B4): `Grammar`, `GrammarPlan`, `GrammarError`,
  `GrammarSink`, `TextSpot`, `Checked`, the member names, and `core.db.SQLName`;
- Sh's refusals of unquoted `| & ; < > ( ) $`, the backtick and list holes
  (B5);
- HTML's strictness, its refusals in tag names, attribute names, raw text and
  comments, and the URL scheme filter (B6).

G-C2 writes these into `syntax-decisions.md` as ratified rows that cite the
P31 approval.

**Decided within the approved scope (2026-11-25).** The owner approved safe
Sh and HTML as part of P31 and asked that these not come back as questions,
so the review's two follow-ups were decided as the safe options:

- **D-SH-ARGV1 = A.** A program-position hole takes only `Path`. A bare
  newline is refused unless it is continued with `\`. `$` and backtick are
  refused inside double quotes. This closes command injection by position
  (I1). It unblocks G-D22b.
- **D-HTML-SCRIPTATTR1 = A.** Holes inside `on*` (including `data-on*`),
  `style`, `srcdoc` and `srcset` are refused at build time. The diagnostic's
  fix points to a class or data attribute. Attribute escaping does not stop
  script or CSS injection, and Go-style context escaping for JS and CSS can
  come later as its own feature. It unblocks G-D20b.

**Compile-cache ruling (Pip, 2026-11-25).** Storing Core's own head grammars
in the Core bodies record is not a compile cache under the performance gate.
The record is generated from Core source when the compiler is built, exactly
as Core derive expansions already are (`CoreBodies.jet:960-1052`). It is
checked by its generator and never reuses work from an earlier user compile,
so every measured compile still does all of its own work from cold.

## 15. Build order (card-ready items, each at most one hour of code)

Conventions:

- **Crit** is the card #7036 criterion row the item serves. **Deps** lists
  the items it needs first.
- **Proof** is the exact command that must pass. A filter matches row IDs
  relative to the repository root (§1); a filter that selects nothing fails.
- `core F` means `jet test Compiler --profile core --filter Core/Grammar/Tests/F.jet`.
- New cases go under `Tests/Cases/GrammarLiterals/`. `Tests/Cases/Grammar/`
  is Jet's own grammar.
- Each item that adds a code also adds its type to
  `Compiler/Prelude/Diagnostics.jet` and runs
  `jet Tools/Agent/DiagnosticRows.jet --write`. Its proof includes
  `jet Tools/Agent/DiagnosticRows.jet --check`.
- Timed commands (`GrammarPair.jet`, `Throughput.jet`) are run by the
  orchestrator, alone. A worker never runs them.

### Phase 0: review

| ID | Item | Files | Crit | Deps | Proof |
|---|---|---|---|---|---|
| G-0 | Land the review as the dated audit, with an empty behavior → source → proof table that items fill in | `Docs/Audits/2026-11-25-grammar-literals-review.md` | 1 | — | `jet Tools/Agent/AgentDocFlags.jet` |

### Phase A: the grammar compiler (pure Core, `core.grammar`)

| ID | Item | Files | Crit | Deps | Proof |
|---|---|---|---|---|---|
| G-A1 | Module skeleton in the `data` layer; `TextSpot`, `GrammarError` (+ Display `line:column`), the `GrammarSink` trait; `Core.rows` rows. `core.grammar` may name only modules of its layer or lower, and never `core.text.html` | `Core/Grammar/Grammar.jet`, `Error.jet`, `Sink.jet`, `Core/package.jet`, `Compiler/Prelude/Core.rows`, `Core/Grammar/Tests/Error.jet` | 2 | — | `core Error` + `jet Tools/Agent/CoreSurfaceLedger.jet --check` |
| G-A2 | Notation lexer: names, `:` `;` `\|` `(` `)` `? * +` `*? +?` `=` `+=` `#` `->` `<…=…>`, both quote styles with escapes, the `i` suffix, classes, `.`, `//` and `/* */`; `{` refused | `Core/Grammar/Notation.jet`, `Tests/Notation.jet` | 2 | G-A1 | `core Notation` |
| G-A3 | Notation parser: parser and token rules, `fragment`, `-> skip`, alternatives with `# Name`, labels, suffixes, groups → `RuleTree`; collect-all `GrammarNotation` | `Notation.jet`, `Core/Grammar/Plan.jet`, `Tests/NotationParse.jet` | 2 | G-A2 | `core NotationParse` |
| G-A4 | Notation parser: `<assoc=right>`, `HOLE<slot=…>`, `import Name ;`; refusal of actions, `options`, `mode` and predicates, with fix texts | same, `Tests/NotationRefusals.jet` | 2 | G-A3 | `core NotationRefusals` |
| G-A5 | Resolve: undefined and duplicate rules, imports and overrides, start rule, casing, the all-or-none `#` rule, label/rule clashes | `Core/Grammar/Analyze.jet`, `Tests/Resolve.jet` | 2 | G-A4 | `core Resolve` |
| G-A6 | Nullable; split of operator-form left recursion; cycle report with path | `Analyze.jet`, `Tests/LeftRecursion.jet` | 2 | G-A5 | `core LeftRecursion` |
| G-A7 | Empty repeats; precedence levels, associativity, the middle-operand rule, operator checks | `Core/Grammar/Precedence.jet`, `Tests/Precedence.jet` | 2 | G-A6 | `core Precedence` |
| G-A8 | FIRST_1/FOLLOW_1 over contextual alphabets; LL(1) decisions | `Core/Grammar/Lookahead.jet`, `Tests/Lookahead.jet` | 2 | G-A6 | `core Lookahead` |
| G-A9 | Lazy SLL(k ≤ 3) deepening at conflicts, the 4,096-sequence cap, the shortest shared sample | `Lookahead.jet`, `Tests/Deepening.jet` | 2 | G-A8 | `core Deepening` |
| G-A10 | Tokens: contextual viability, literal-wins, longest match, non-greedy before a literal, clashes | `Core/Grammar/Tokens.jet`, `Tests/Tokens.jet` | 2 | G-A8 | `core Tokens` |
| G-A11 | Shapes: types, cases and fields per §4 | `Core/Grammar/Shape.jet`, `Tests/Shape.jet` | 2 | G-A5 | `core Shape` |
| G-A12 | Printer table: separators, canonical token text, grouping alternative | `Core/Grammar/Print.jet`, `Tests/Print.jet` | 2 | G-A10, G-A7 | `core Print` |
| G-A13 | `GrammarPlan` as compile-time-representable data: class tables, decision tables | `Plan.jet`, `Tests/Plan.jet` | 2 | G-A9, G-A10 | `core Plan` |
| G-A14 | Keyword perfect hash built into the plan | `Core/Grammar/Keywords.jet`, `Tests/Keywords.jet` | 2 | G-A13 | `core Keywords` |
| G-A15 | Run-time support: cursor, class runs, keyword lookup, `GRAMMAR_MAX_DEPTH`, error text, `line:column` on error, resync bookkeeping | `Core/Grammar/Runtime.jet`, `Tests/Runtime.jet` | 2 | G-A1 | `core Runtime` |
| G-A16 | Budget test: a generated 400-rule grammar analyzes, and its step count is recorded against 10,000,000 | `Tests/Budget.jet` | 2 | G-A9..A14 | `core Budget` |
| G-A17 | `Grammar` implements `CheckedText`: `check` = analyze → `Checked<GrammarPlan, Never>`; `accepts` refuses every hole | `Grammar.jet`, `Tests/Head.jet` | 2 | G-A16, G-B1 | `core Head` |

### Phase B: the compiler

G-B1 to G-B3g land as one change (§11 item 1).

| ID | Item | Files | Crit | Deps | Proof |
|---|---|---|---|---|---|
| G-B1 | Prelude trait amendment (`Slot`, `Shape`, `Checked`, `accepts`, signatures) and the trait contract check | `Compiler/Prelude/Core/prelude.jet`, `OwnedTraitContracts.jet` | 2 | — | `jet test Compiler --profile cases --filter Tests/Cases/CheckedText/malformed_checked_text_impl_reports_the_trait_contract` |
| G-B2 | Literal lowering: `check(parts)`, `Shape` embedded through the `BodyExpression` route, slot to `encode_hole`, `build(shape, holes)`; the check always runs | `Constructors.jet` (2023-2177), `ComptimeFunctions.jet` | 2 | G-B1 | `jet test Compiler --profile cases --filter Tests/Cases/CheckedText/checked_text_hooks_check_encode_and_build` |
| G-B2b | `from`, `raw` and run-time paths call `check([text])`; E0907 text | `Constructors.jet` (1862-2021), `Compiler/Prelude/Diagnostics.jet` | 2 | G-B2 | `jet test Compiler --profile cases --filter Tests/Cases/CheckedText/dynamic_checked_text_from_keeps_the_shared_error_report` + `jet Tools/Agent/DiagnosticRows.jet --check` |
| G-B2c | The other readers of the contract | `Effects/Declarations.jet`, `ComptimePrecheck.jet`, `TypeRelations.jet`, `JetSemIndex/…/Symbols.jet`, `GateLedger.jet` | 2 | G-B2b | `jet test Compiler --profile cases --filter Tests/Cases/Semindex/checked_text_contract_is_one_tooling_fact` |
| G-B3a | Core impls SQL, HTML, Sh on the new signatures, same behavior | `Core/DB/DB.jet`, `Core/Text/HTML.jet`, `Core/Process/Process.jet` | 2 | G-B2c | `jet test Compiler --profile cases --filter Tests/Cases/CheckedText/typed_sql_and_html_blocks_run_on_every_tier` + `… --filter Tests/Cases/CoreLibPartsPlatform/core_process_sh_typed_text_keeps_each_hole_one_argv_item` |
| G-B3b | Core impls URL, DateTime, Path | `Core/Net/URL.jet`, `Core/Time/Time.jet`, `Core/Files/Files.jet` | 2 | G-B2c | `jet test Compiler --profile cases --filter Tests/Cases/DSLBlocks/boundary_typed_heads_validate_and_compile` + `… --filter Tests/Cases/LanguageFeatures/boundary_typed_heads_keep_one_meaning_across_tiers` |
| G-B3c | Examples | the seven `Examples/Text/CheckedText/` files of §11 item 1, `UserCheckedTextShape.jet`, `CypherHead/Cypher/cypher.jet` | 2 | G-B3a | `jet test Compiler --profile golden Examples/Text/CheckedText` + `… golden Examples/Types/Traits/UserCheckedTextShape` + `… golden Examples/Modules/Packages/CypherHead` |
| G-B3d | `Tests/Cases/CheckedText/` fixtures | the 10 fixtures | 2 | G-B3a | `jet test Compiler --profile cases --filter Tests/Cases/CheckedText/` |
| G-B3e | Crossings, CLI parity, Fmt, JetDoc | `CheckedTextCrossings/`, `ComptimeCLIParity/` (`tooling_checked_text_type`), `Fmt/FmtPreservesCheckedTextType`, `JetDoc/CheckedTextContractIsRenderedInGeneratedDocs.fixture` | 2 | G-B3a | `… cases --filter Tests/Cases/CheckedTextCrossings/`, `… --filter Tests/Cases/ComptimeCLIParity/tier_parity_examples_run_through_aot_jit_and_o0`, `… --filter Tests/Cases/Fmt/fmt_preserves_checked_text_type`, `… --filter Tests/Cases/JetDoc/checked_text_contract_is_rendered_in_generated_docs` |
| G-B3f | LSP, LanguageFeatures, Reflect, Semindex | the four fixtures and the LSP case's embedded source | 2 | G-B3a | `… cases --filter Tests/Cases/LSP/lsp_checked_text_contract_survives_editor_navigation`, `… --filter Tests/Cases/LanguageFeatures/impl_method_inherits_trait_effect_row_on_every_tier`, `… --filter Tests/Cases/Reflect/checked_text_contract_is_reflected_on_the_nominal_type`, `… --filter Tests/Cases/Semindex/checked_text_contract_is_one_tooling_fact` |
| G-B3g | UI fixtures and spec text | the `Tests/UI` files of §11 item 1, `Docs/Spec/spec.md:88-116` | 2, 5 | G-B3a | `jet test Compiler --profile ui Tests/UI/checked_text_` + `… ui Tests/UI/PackageHead` + `… ui Tests/UI/sink_needs_checked_text.jet` + `… ui Tests/UI/typed_literal_text_body_needs_checked_text.jet` + `jet Tools/Agent/AgentDocFlags.jet --policy-only` |
| G-B4 | `accepts` per hole at build time; registers `Jet.CheckedTextHoleSlot` | `Constructors.jet`, `Compiler/Prelude/Diagnostics.jet`, `Tests/UI/checked_text_hole_slot.jet` | 3 | G-B3g | `jet test Compiler --profile ui Tests/UI/checked_text_hole_slot.jet` + `jet Tools/Agent/DiagnosticRows.jet --check` |
| G-B5 | `TextSpot` → span through part spans and the dedent map | `Constructors.jet`, `Pipeline.jet`, `Tests/UI/checked_text_error_points_at_line_and_column.jet` | 2 | G-B3g | `jet test Compiler --profile ui Tests/UI/checked_text_error_points_at_line_and_column.jet` |
| G-B6 | List errors with `Diagnostic` codes; delete the reason sniffing; collect-all through `graph.keep_going` | `Pipeline.jet`, `Tests/UI/checked_text_reports_every_literal.jet` | 2 | G-B5 | `jet test Compiler --profile ui Tests/UI/checked_text_reports_every_literal.jet` |
| G-B7 | Declaring-binding detection by `use`-table identity; casing exemption; `Spellings.jet` row | the module-binding registration in `Sema/Registration/Items.jet`, `Spellings.jet`, `Tests/Cases/GrammarLiterals/declaring_head_is_found_by_identity.case` + fixture (alias; a user type named `Grammar` that declares nothing) | 2 | G-A17 | `jet test Compiler --profile cases --filter Tests/Cases/GrammarLiterals/declaring_head_is_found_by_identity` |
| G-B8a | Pre-registration batch: gather declaring literals after load; one compile-time request over the `core.grammar` closure; plans back as values. **If the child cannot serve a request before registration, stop and return the design to review** | `Pipeline.jet`, `ComptimeFunctions.jet`, `Tests/Cases/GrammarLiterals/plan_is_evaluated_before_registration.case` + fixture | 2 | G-B7 | `jet test Compiler --profile cases --filter Tests/Cases/GrammarLiterals/plan_is_evaluated_before_registration` |
| G-B8b | `grammar_plans` in `SemaRegistrationModuleInput`, handed to registration | `Sema/Registration/Items.jet`, `Pipeline.jet` | 2 | G-B8a | same case, extended to O0/O1/O2 and `jet build` steps |
| G-B8c | Core modules' plans and items recorded in and read from the Core bodies file under a new key | `Sema/CoreBodies.jet`, `Tests/Cases/GrammarLiterals/core_grammar_plan_comes_from_the_core_record.case` | 2 | G-B9 | `jet test Compiler --profile cases --filter Tests/Cases/GrammarLiterals/core_grammar_plan_comes_from_the_core_record` |
| G-B9 | `GrammarItems`: types (structs, enums, Slot enum, constants) with the binding's visibility, through `sema_user_rule_accept` | `Sema/Registration/GrammarItems.jet`, `Tests/Cases/GrammarLiterals/types_are_generated.case` | 2 | G-B8b, G-A11 | `jet test Compiler --profile cases --filter Tests/Cases/GrammarLiterals/types_are_generated` |
| G-B10a | `GrammarItems`: sequence, choice, optional, repeat and list labels, calling `Runtime.jet` | `GrammarItems.jet`, `Tests/Cases/GrammarLiterals/parse_shapes.case` | 2 | G-B9, G-A15 | `jet test Compiler --profile cases --filter Tests/Cases/GrammarLiterals/parse_shapes` |
| G-B10b | `GrammarItems`: contextual token dispatch, keyword lookup, `EOF`, depth guard, errors; the INI golden example | `GrammarItems.jet`, `Examples/Text/Grammar/Ini.jet` (+ `.out`) | 2, 5 | G-B10a, G-A14 | `jet test Compiler --profile golden Examples/Text/Grammar/Ini` |
| G-B11 | `GrammarItems`: precedence climbing; the Calc golden example | `GrammarItems.jet`, `Examples/Text/Grammar/Calc.jet` (+ `.out`) | 2, 5 | G-B10b, G-A7 | `jet test Compiler --profile golden Examples/Text/Grammar/Calc` |
| G-B12a | `print`/`print_into` with token-field checks; round-trip law | `GrammarItems.jet`, `Tests/Cases/GrammarLiterals/print_round_trips.case` | 2 | G-B10b, G-A12 | `jet test Compiler --profile cases --filter Tests/Cases/GrammarLiterals/print_round_trips` |
| G-B12b | Parenthesizing hand-built values through the grouping alternative; an injected-token value returns `GrammarError` | `GrammarItems.jet`, `Tests/Cases/GrammarLiterals/print_checks_hand_built_values.case` | 2 | G-B12a, G-B11 | `jet test Compiler --profile cases --filter Tests/Cases/GrammarLiterals/print_checks_hand_built_values` |
| G-B13a | `parse_into` (sink) | `GrammarItems.jet`, `Tests/Cases/GrammarLiterals/sink_events.case` | 2 | G-B10b | `jet test Compiler --profile cases --filter Tests/Cases/GrammarLiterals/sink_events` |
| G-B13b | `check_parts` with `HOLE` slots, and resync | `GrammarItems.jet`, `Tests/Cases/GrammarLiterals/holes_and_resync.case` | 2 | G-B13a | `jet test Compiler --profile cases --filter Tests/Cases/GrammarLiterals/holes_and_resync` |
| G-B14 | Registers `GrammarLeftRecursive`, `GrammarAmbiguous`, `GrammarEmptyRepeat`, `GrammarTooComplex`; UI snapshots | `Compiler/Prelude/Diagnostics.jet`, `Core/Grammar/Error.jet`, `Tests/UI/grammar_left_recursive.jet`, `grammar_ambiguous.jet`, `grammar_empty_repeat.jet`, `grammar_too_complex.jet` | 2 | G-B9 | `jet test Compiler --profile ui Tests/UI/grammar_` + `jet Tools/Agent/DiagnosticRows.jet --check` |
| G-B15 | Registers `GrammarNotation`, `GrammarUndefinedRule`, `GrammarStartRule`; UI snapshots | same files, three `Tests/UI/grammar_*.jet` | 2 | G-B14 | same |
| G-B16 | Registers `GrammarTokenClash`, `GrammarFieldShape`, `GrammarUnprintable`; UI snapshots | same files, three `Tests/UI/grammar_*.jet` | 2 | G-B14 | same |
| G-B17 | Tier parity: one grammar with holes and a sink at O0, O1, O2, `jet build` and web, as `checked_text_acceptance_matches_comptime_aot_jit_o0_and_web` does (I9; the first draft's "JIT, interpreter" tiers do not exist) | `Tests/Cases/GrammarLiterals/grammar_matches_every_tier.case` + fixture | 5 | G-B13b | `jet test Compiler --profile cases --filter Tests/Cases/GrammarLiterals/grammar_matches_every_tier` |
| G-P1 | Compile-cost measurement (orchestrator) | — | 2 | G-B8c, G-B13b | `JET=<candidate jet> jet Tools/Perf/CompileThroughput/Throughput.jet --against <seed jet> --modes check,O0` → `speed gate: PASS` |

### Phase C: examples, spec, editors, harness

| ID | Item | Files | Crit | Deps | Proof |
|---|---|---|---|---|---|
| G-C1 | LogLine golden example | `Examples/Text/Grammar/LogLine.jet` (+ `.out`) | 5 | G-B10b | `jet test Compiler --profile golden Examples/Text/Grammar/LogLine` |
| G-C2 | Spec: "Grammar literals" section; ratified rows citing P31 for B1–B6; the amended D-TEXTHEAD-HOLES1, D-TEXTHEAD-TYPE1 and D-META-LADDER1 text; spec.md:97 `&` | `Docs/Spec/spec.md`, `Docs/Spec/syntax-decisions.md` | 5 | G-B17 | `jet Tools/Agent/AgentDocFlags.jet --policy-only` + `jet Tools/Agent/DiagnosticRows.jet --check` |
| G-C3 | Reference: the `core.grammar` section | `Docs/Spec/Reference/core-library.md` | 5 | G-A17 | `jet Tools/Agent/AgentDocFlags.jet --policy-only` + `jet Tools/Agent/CoreSourceRows.jet --check` |
| G-C4 | Editor grammars: regenerate after the `Spellings.jet` row; add an embedded-notation rule for `Grammar{` bodies by hand | `Tools/Editors/Vscode/Syntaxes/jet.tmLanguage.json`, `Tools/Editors/Zed/languages/jet/highlights.scm`, `Tools/Editors/TreeSitter/grammar.js` | 5 | G-B7 | `jet self devtools grammars`, then `jet test Compiler --profile cases --filter Tests/Cases/Grammar/editor_grammars_match_generated_sections` [the hand-written highlight colors have no command proof] |
| G-C5 | `GrammarPair.jet`, the manifest's `grammar_pairs` list, the `ini` pair | `Tools/Perf/GrammarPair.jet`, `Tools/Perf/GrammarPair/Ini/*`, `Tools/Gauntlet/measurement-manifest.json` | 4 | G-B13a | `jet Tools/Perf/GrammarPair.jet --pair ini` (orchestrator) |

### Phase D: heads

SQL grammar (criterion 3). `sqlg` means
`jet test Compiler --profile core --filter Core/DB/Tests/SQLGrammar.jet`.

| ID | Item | Files | Crit | Deps | Proof |
|---|---|---|---|---|---|
| G-D1 | Tokens, keywords (`'…'i`), identifiers, quoted names, literals, comments; `fallback_keyword` = the non-zero `yyFallback` entries of `ParseTables.jet:6013`, with a test that the two lists match | `Core/DB/Syntax/SQL.jet`, `Core/DB/Tests/SQLGrammar.jet` | 3 | G-B13b | `sqlg` |
| G-D2 | Expressions 1: operator alternatives in SQLite precedence, unary, literals, column refs, `HOLE<slot=Operand>` | same | 3 | G-D1 | `sqlg` |
| G-D3 | Expressions 2: `CASE`, `CAST`, `[NOT] IN`, `[NOT] BETWEEN` (middle operand), `[NOT] LIKE/GLOB/REGEXP/MATCH … ESCAPE`, `IS [NOT] [DISTINCT FROM]`, `COLLATE`, `ISNULL/NOTNULL`, calls with `FILTER`/`OVER`, `RAISE`, subqueries | same | 3 | G-D2 | `sqlg` |
| G-D4 | `SELECT` core: result columns, `FROM`, joins, `WHERE`, `GROUP BY`, `HAVING` | same | 3 | G-D3 | `sqlg` |
| G-D5 | `SELECT` rest: compound, `VALUES`, `ORDER BY`, `LIMIT`, `WITH [RECURSIVE]`, `WINDOW` | same | 3 | G-D4 | `sqlg` |
| G-D6 | `INSERT`, upsert, `RETURNING` | same | 3 | G-D5 | `sqlg` |
| G-D7 | `UPDATE`, `DELETE` (with `FROM`, `ORDER BY`, `LIMIT`) | same | 3 | G-D6 | `sqlg` |
| G-D8 | `CREATE TABLE`: columns, type names, column and table constraints, generated columns, `AS SELECT` | same | 3 | G-D5 | `sqlg` |
| G-D9 | `CREATE INDEX/VIEW/TRIGGER/VIRTUAL TABLE` | same | 3 | G-D8 | `sqlg` |
| G-D10 | `ALTER`, `DROP`, transactions, savepoints, `PRAGMA`, `ATTACH`/`DETACH`, `VACUUM`, `ANALYZE`, `REINDEX`, `EXPLAIN [QUERY PLAN]` | same | 3 | G-D8 | `sqlg` |
| G-D11 | The corpus: every SQL text in `Core/DB/Tests/*.jet` and in `Tests/Cases/DBPolicy/`, `Tests/Cases/CoreLib/db_*` fixtures; the grammar as a real `Grammar` literal compiles within the step limit | `Core/DB/Tests/SQLGrammarCorpus.jet` | 3 | G-D7, G-D9, G-D10 | `jet test Compiler --profile core --filter Core/DB/Tests/SQLGrammarCorpus.jet` |
| G-D12 | Differential test: the grammar accepts and rejects exactly the corpus statements the Lemon parser does | `Core/DB/Tests/SQLGrammarDifferential.jet` | 3 | G-D11 | `jet test Compiler --profile core --filter Core/DB/Tests/SQLGrammarDifferential.jet` |
| G-D13 | SQL head on the grammar: `SQLSlot`, `SQLName`, `SQLHole`, `accepts` (lists refused), reserved words, `build` | `Core/DB/DB.jet`, `Core/DB/Syntax/SQL.jet`, `Tests/UI/sql_head_rejects_malformed_query.jet` | 3 | G-D12, G-B4 | `jet test Compiler --profile ui Tests/UI/sql_head_rejects_malformed_query.jet` |
| G-D14 | SQL head cases: name vs operand holes, a hole in a string literal, a list hole, a reserved word, every tier | `Tests/Cases/GrammarLiterals/sql_head_holes_typed_by_position.case`, `Tests/UI/sql_head_name_hole_needs_sqlname.jet` | 3 | G-D13 | `jet test Compiler --profile cases --filter Tests/Cases/GrammarLiterals/sql_head_holes_typed_by_position` + `jet test Compiler --profile ui Tests/UI/sql_head_name_hole_needs_sqlname.jet` |
| G-D15 | Policy scoping on the tree 1: one statement, table and schema | `Core/DB/DB.jet` | 4 | G-D13 | `jet test Compiler --profile cases --filter Tests/Cases/DBPolicy/` |
| G-D16 | Policy scoping 2: insert shape and update-owner; delete `DB.jet:404-724, 3419-3460`; the `sql_policy` pair | `Core/DB/DB.jet`, `Tools/Perf/GrammarPair/SQLPolicy/*` | 4 | G-D15 | `jet test Compiler --profile cases --filter Tests/Cases/DBPolicy/` + `jet Tools/Perf/GrammarPair.jet --pair sql_policy` |
| G-D17 | Analytics subset as a shape check; delete the `Core/DB/SQL.jet` parser | `Core/DB/SQL.jet`, `Core/DB/Query.jet` | 4 | G-D13 | `jet test Compiler --profile cases --filter Tests/Cases/CoreLib/db_checked_sql_execute_uses_typed_sql` |
| G-D18.1–G-D18.16 | The engine sink, one parse.y family per item, each with a per-family differential test against the Lemon-built AST: (1) cmd and transactions; (2) create-table head; (3) column defs and type names; (4) column constraints; (5) table constraints; (6) drop and the select root; (7) result columns; (8) from and joins; (9) group, order and limit; (10) insert, upsert, returning; (11) update and delete; (12) expressions 1; (13) expressions 2; (14) index and view; (15) triggers; (16) pragma, attach, vacuum, analyze, reindex, alter, window defs | `Core/DB/Engine/GrammarSink.jet`, `Core/DB/Engine/Build.jet`, `Core/DB/Tests/SQLGrammarSink.jet` | 4 | G-D12 | `jet test Compiler --profile core --filter Core/DB/Tests/SQLGrammarSink.jet` |
| G-D19 | The `sql_engine` pair and G-P2. On a pass: delete `Engine/Parse.jet`, `ParseTables.jet` and the `Tokenize.jet` hand lexer; delete the tests that compare against them (the `yyFallback` cross-check of G-D1, `SQLGrammarDifferential.jet`, the Lemon side of `SQLGrammarSink.jet`), keeping their corpus as plain grammar tests; re-point `nested_parse`, `take_parse` and `parse_schema` | `Core/DB/Engine/*`, `Core/DB/Tests/SQLGrammar*.jet`, `Tools/Perf/GrammarPair/SQLEngine/*` | 4 | G-D18.16 | `jet Tools/Perf/GrammarPair.jet --pair sql_engine` + G-P2 (the G-P1 command) + `jet test Compiler --profile core --filter Core/DB/Tests/SQLKernel.jet` + `… --filter Core/DB/Tests/SQLGrammarCorpus.jet` |

The other heads:

| ID | Item | Files | Crit | Deps | Proof |
|---|---|---|---|---|---|
| G-D20a | HTML grammar: elements, void elements, raw-text elements, comments, attributes; the close-tag check over the tree | `Core/Text/Syntax/HTML.jet`, `Core/Text/Tests/HTMLGrammar.jet` | 6 | G-B13b | `jet test Compiler --profile core --filter Core/Text/Tests/HTMLGrammar.jet` |
| G-D20b | HTML head: slot refinement by attribute (Go's URL attribute table), the escaping per slot, the scheme filter for `String` and `URL` holes, prefix percent-encoding, the B8 refusals; delete `HTML.jet:147-205` | `Core/Text/HTML.jet`, `Tests/UI/html_head_rejects_broken_markup.jet` | 6 | G-D20a, G-B4, B8 | `jet test Compiler --profile ui Tests/UI/html_head_rejects_broken_markup.jet` |
| G-D21 | HTML cases: escaping for each slot, `javascript:` in a `URL` value, every tier | `Tests/Cases/GrammarLiterals/html_head_escapes_by_position.case` | 6 | G-D20b | `jet test Compiler --profile cases --filter Tests/Cases/GrammarLiterals/html_head_escapes_by_position` + `… --filter Tests/Cases/CheckedText/html_composition_matches_aot_jit_o0_and_web` |
| G-D22a | Sh grammar and head: words, pieces, argv shape, B5 refusals; delete `sh_split_into` | `Core/Process/Syntax/Sh.jet`, `Core/Process/Process.jet`, `Tests/UI/sh_head_rejects_pipe.jet` | 7 | G-B13b, G-B4 | `jet test Compiler --profile ui Tests/UI/sh_head_rejects_pipe.jet` |
| G-D22b | The B7 rules: `Path`-only program hole, newline, `$` and backtick in double quotes | `Core/Process/Syntax/Sh.jet`, `Core/Process/Process.jet`, `Tests/UI/sh_head_program_hole_needs_path.jet` | 7 | G-D22a, B7 | `jet test Compiler --profile ui Tests/UI/sh_head_program_hole_needs_path.jet` |
| G-D23 | Sh cases: `a{x}b`, quotes, an empty hole, `*` stays literal | `Tests/Cases/GrammarLiterals/sh_head_one_hole_one_argv_item.case` | 7 | G-D22a | `jet test Compiler --profile cases --filter Tests/Cases/GrammarLiterals/sh_head_one_hole_one_argv_item` + `… --filter Tests/Cases/CoreLibPartsPlatform/core_process_sh_typed_text_keeps_each_hole_one_argv_item` |
| G-D24a | URL grammar 1: scheme, authority, userinfo, host, port | `Core/Net/Syntax/URL.jet`, `Core/Net/Tests/URLGrammar.jet` | 8 | G-B13b | `jet test Compiler --profile core --filter Core/Net/Tests/URLGrammar.jet` |
| G-D24b | URL grammar 2: path, query, fragment; head slots (§9.4); delete `url_hole_marker` and `url_literal_reason` | same, `Core/Net/URL.jet` | 8 | G-D24a, G-B4 | `jet test Compiler --profile cases --filter Tests/Cases/DSLBlocks/boundary_typed_heads_validate_and_compile` |
| G-D25a | Run-time `URL.parse` on the grammar; delete `parse_recipe` | `Core/Net/URL.jet` | 4, 8 | G-D24b | `jet test Compiler --profile core --filter Core/Net/Tests/URLGrammar.jet` |
| G-D25b | `parse_qsl`, `parse_port` onto rules; the `url` pair | `Core/Net/URL.jet`, `Tools/Perf/GrammarPair/URL/*` | 4, 8 | G-D25a | `jet Tools/Perf/GrammarPair.jet --pair url` |
| G-D26 | RFC 3339 grammar and the DateTime head (`Shape` = `DateTime`); delete the re-parse in `build` | `Core/Time/Syntax/RFC3339.jet`, `Core/Time/Time.jet` | 8 | G-B13b | `jet test Compiler --profile cases --filter Tests/Cases/LanguageFeatures/boundary_typed_heads_keep_one_meaning_across_tiers` |
| G-D27 | Time parsers on the grammar (date, time, offset, ISO week); `rfc3339` pair | `Core/Time/Time.jet`, `Tools/Perf/GrammarPair/RFC3339/*` | 4, 8 | G-D26 | `jet Tools/Perf/GrammarPair.jet --pair rfc3339` |
| G-D28 | The Path head on parts and slots (`Component`) | `Core/Files/Files.jet`, `Core/Files/Syntax/Path.jet` | 8 | G-B3b | `jet test Compiler --profile cases --filter Tests/Cases/DSLBlocks/boundary_typed_heads_validate_and_compile` |
| G-D29a | Regex grammar 1: atoms, classes, escapes, Unicode classes | `Core/Regex/Syntax/Regex.jet`, `Core/Regex/Tests/RegexGrammar.jet` | 8 | G-B13b | `jet test Compiler --profile core --filter Core/Regex/Tests/RegexGrammar.jet` |
| G-D29b | Regex grammar 2: groups, lookaround, named groups, quantifiers and counts; the sink into the existing compiler | same, `Core/Regex/Regex.jet` | 8 | G-D29a | `jet test Compiler --profile golden Examples/Text/RegexSyntax` |
| G-D30 | `Regex` implements `CheckedText`; delete the sema Regex path; retire E0152 | `Core/Regex/Regex.jet`, `Constructors.jet`, `Compiler/Prelude/Diagnostics.jet` | 8 | G-D29b | `jet test Compiler --profile cases --filter Tests/Cases/DSLBlocks/regex_typed_head_owns_backslashes_but_rejects_holes` + `jet Tools/Agent/DiagnosticRows.jet --check` |
| G-D31 | `regex_syntax` pair; delete the 14 hand `parse_*` | `Core/Regex/Regex.jet`, `Tools/Perf/GrammarPair/RegexSyntax/*` | 4 | G-D30 | `jet Tools/Perf/GrammarPair.jet --pair regex_syntax` |
| G-D32 | Golden example of the heads (SQL, HTML, Sh holes) on every tier | `Examples/Text/Grammar/Heads.jet` (+ `.out`) | 5 | G-D14, G-D21, G-D23 | `jet test Compiler --profile golden Examples/Text/Grammar/Heads` |

### Phase E: the other Core parsers (criterion 4)

Each row is at least two items:

- **a:** the grammar and its Core tests in `Core/<Area>/Tests/<Name>Grammar.jet`.
  Proof: `jet test Compiler --profile core --filter Core/<Area>/Tests/<Name>Grammar.jet`.
- **b:** the cutover, which re-points callers, deletes the hand parser and adds
  the pair. Proof: `jet Tools/Perf/GrammarPair.jet --pair <name>` plus the
  same core filter.

A row marked ×N has N grammar items (a1…aN).

| ID | Grammar | Files | Deps |
|---|---|---|---|
| G-E1 | `Ini` | `Core/Encoding/Syntax/Ini.jet`, `Core/Encoding/Ini.jet` | G-C5 |
| G-E2 | `Json` (sink to `Dyn`; depth and duplicate keys in the sink); re-point `JSONL.jet` and `Router.jet` | `Core/Encoding/Syntax/JSON.jet`, `JSON.jet`, `JSONL.jet`, `Core/Web/Router.jet` | G-C5 |
| G-E3 | `Json5` imports `Json`; delete `Func.jet` `json_parse_*` | `Core/Encoding/Syntax/JSON5.jet`, `Core/DB/Engine/Func.jet` | G-E2 |
| G-E4 | `Toml` ×3 (keys and tables; values and arrays; inline tables and dates) | `Core/Encoding/Syntax/TOML.jet`, `TOML.jet` | G-D27 |
| G-E5 | `Xml` ×3 (elements; attributes and entities through the sink; prolog and DTD subset) | `Core/Encoding/Syntax/XML.jet`, `XMLEngine.jet`, `XMLKernel.jet`, `XML.jet` | G-C5 |
| G-E6 | `Ip`; delete the `Net.jet` duplicates | `Core/Net/Syntax/IP.jet`, `IP.jet`, `Net.jet` | G-C5 |
| G-E7 | `Mime` | `Core/Net/Syntax/MIME.jet`, `MIME.jet` | G-C5 |
| G-E8 | `HttpHead` ×2 and `Route` ×1 | `Core/HTTP/Syntax/Head.jet`, `HTTP.jet`, `Client.jet`, `Server.jet` | G-C5 |
| G-E9 | `Cron`, `SyncDocument` (one row each) | `Core/App/Syntax/*.jet`, `App.jet` | G-C5 |
| G-E10 | `LogFilter`, `Uuid`, `Pem`, `PolicyJoin` (one row each) | the matching `Syntax/*.jet` and sources | G-C5 |
| G-E11 | `Xkb`, `ServiceRecord` (one row each) | `Core/UI/Syntax/Xkb.jet`, `Core/Service/Syntax/Record.jet` and sources | G-C5 |
| G-E12 | Delete the duplicate integer parsers; call `core.text.parse` (one item per two files) | `Log.jet`, `Crypto.jet`, `Service.jet`, `Browser.jet`, `HTTP/Client.jet`, `Encoding/Stream.jet`, Term, Net, URL, Archive | — (`jet test Compiler --profile core --filter <file>`) |
| G-E13 | Record the kept parsers with their reasons in the audit | `Docs/Audits/2026-11-25-grammar-literals-review.md` | G-0 (`jet Tools/Agent/AgentDocFlags.jet`) |

Critical path:

1. G-A1..A17;
2. G-B1..B3g (one landing), then G-B4..B13b, then G-P1;
3. G-C1, G-C5;
4. G-D1..D14 (criterion 3), then D20..D23 (criteria 6 and 7);
5. everything else in parallel.

## 16. Claims not verified

- **Pre-registration staging (G-B8a)** [INFERENCE]. Today the compile-time
  child serves requests raised while the whole graph is checked. That it can
  serve a batch whose closure is only `core.grammar`, before registration, is
  untested. G-B8a is the probe, and its failure sends the design back to
  review.
- **Core bodies record as compile cache** [UNVERIFIED]. Whether the
  performance gate's "no compile cache" rule counts the Core bodies file (and
  the derive records already in it) is not ruled anywhere I found. G-P1
  measures the result either way.
- **`TypeId` at compile time.** `TypeId` exists as compile-time data. Marker
  rules receive one per argument (`spec.md:142`), and settings carry one
  (`spec.md:215`). Handing one per hole to the CheckedText request is new
  [INFERENCE]; G-B4 settles it.
- **Compile-time representability.** The value algebra is verified, and
  `GrammarPlan` is designed inside it (§2.2). `DateTime` is
  `{secs: Int, nanos: Int, leap_second: Bool}` (`Core/Time/Time.jet:979-982`),
  which fits the algebra. `Regex` (`Core/Regex/Regex.jet:93-117`) and its
  `Dfa` (1459-1475) are structs of lists, numbers, `Bool`s and strings, with
  no map or handle. Two points are [INFERENCE], and G-D30 settles them:
  - whether lists of sized numbers (`[I64]`, `[U8]`) travel as the algebra's
    `.Int`;
  - whether `priv` fields embed.
  If either fails, Regex keeps a list-only `Shape` (the pattern and flags) and
  compiles once per literal at run time.
- **Core.rows and generated items.** Whether the Core declaration gate accepts
  `priv` items generated in a Core module without `owns` rows is
  [INFERENCE]. G-B8c finds out.
- **Examples** were not compiled.
- **Performance.** Direct-coded SLL(k) matching or beating Lemon's tables, the
  JSON and XML readers and the HTTP head reader is a plan; each pair decides.
  SWAR support in codegen was not checked, and nothing in Core uses SWAR
  today.
- **SQL in SLL(3).** That SQLite's syntax fits SLL(3) with explicit fallback
  is [INFERENCE]. Candidates for factoring are type names followed by column
  constraints, `(` (subquery, row value or expression), and `t.*` against
  `t.col`. G-D12 decides.
- **Peers.** The Jai claims have no official public source. The W3C notation
  section was read through the W3C site. The Bison, ANTLR (including
  grammars-v4 `SQLiteParser.g4`), Lark and pest pages and Go's
  `html/template` `attr.go` and `url.go` were read on 2026-11-25.

## 17. Review (ReviewGrammar, 2026-11-25)

An adversarial review of the first draft against the syntax registry, the
spec, the compiler and Core code at dev `e1d1b579653`, Tower decisions
(read-only), and the owner's 2026-11-25 direction on notation and approvals.
Repairs are applied in the sections above.

### Findings and repairs

**Critical**

- **R-C1 — staging costs a second whole compile.**
  - Finding: the draft staged through the root `#loop` wait. With
    `loop_values` set, `jet_driver_compile_stages` runs a full probe compile,
    reloads, and compiles again (`Pipeline.jet:~5959-5971`). The wait check
    covers every loaded module, Core included. If SQL, HTML, URL, Time and
    Regex became grammars, almost every program would pay two cold compiles,
    against the 30M lines/s target. The draft had no compile-time cell.
  - Repair (§2.2 items 3–4, §6, §7, G-B8a–c, G-P1/P2):
    - a pre-registration batch over closed literals;
    - Core plans in the Core bodies record, following the derive pattern;
    - a required compile-throughput gate;
    - G-B8a as a probe that sends the design back to review if it fails.
- **R-C2 — the CheckedText change breaks `from`, `raw` and the run-time
  check, and the cutover list was a fraction of the callers.**
  - Finding: D-TEXTHEAD-TYPE1 binds `Type.from` to the same `check(text)`
    (E0907, `Constructors.jet:1862-2021`). The draft never said what `from`
    calls. It missed eight compiler sites, the Cypher package example, and
    about 25 case and UI fixtures (Crossings, CLI parity, Fmt, JetDoc, LSP,
    LanguageFeatures, Reflect, Semindex, UI). SQL/HTML/Sh skip the check
    today (`Error: Never`).
  - Repair (§2.3, §11 item 1, G-B1–B3g):
    - `from` = `check([text])`, and the check always runs;
    - the full list, split into eight items that land as one change.
- **R-C3 — HTML holes in script, style and URL contexts.**
  - Finding: the draft had no rule for `on*`, `style`, `srcdoc` or `srcset`,
    where attribute escaping does not stop script or CSS injection (I1). It
    also used a `URL` hole "as is", though a `URL` can hold `javascript:`.
    The URL attribute list was short.
  - Repair (§8.5, G-D20b, ballot B8):
    - Go's URL attribute table;
    - the scheme filter applies to `URL` values too;
    - a hole after a fixed prefix is percent-encoded;
    - holes are refused in script and style attributes.
- **R-C4 — SQL `Value` and `Name` slots collide in expressions.**
  - Finding: both were viable wherever an identifier and an operand may
    stand. That is an LL conflict in the grammar itself, and a column-name
    hole could have been bound as a value.
  - Repair (§8.4, §9.1): one `Operand` slot chosen by the hole's type, and
    `Name` only where only an identifier may stand.

**High**

- **R-H1 — "type identity, not a name sniff" and "ordinary Core type" were
  not true as drafted.**
  - Finding: `DECLARING_TYPED_HEADS: ["Grammar"]` is a list of spellings, like
    `BOUNDARY_TYPED_HEADS`. It breaks under aliasing and catches user types
    named `Grammar`. The compiler-side builder held grammar meaning. The
    draft did not face D-TEXTHEAD-TYPE1's rejection of dedicated declaration
    mechanisms, or D-META-LADDER1's expert gating of types from text.
  - Repair (§2.1, §2.2 items 2 and 5, §14):
    - identity through the `use` table;
    - a thin builder, with all meaning in `Runtime.jet`;
    - both decisions recorded as amended under the owner's P31 approval of
      B1 (G-C2).
- **R-H2 — the notation was a Jet invention.**
  - Finding: the owner directed the industry-standard notation.
  - Repair (§3, §4, §8): ANTLR 4/Bison `name : … ;`, both quote styles, `|`,
    `* + ?`, groups, W3C/Lark classes, ANTLR labels and `# Alt`, ANTLR
    `-> skip`, `fragment`, `import`, `EOF` and operator precedence, and Lark
    `"x"i`.
    - Removed: `%` separators, back-references, `operators` tables,
      `int/float` ranges, `extends`/`|=`, the `lookahead`/`depth` options
      (whose braces could not be written), and `any/end/eol`.
    - The one Jet addition is `HOLE<slot=…>`, in ANTLR's element-option
      syntax.
- **R-H3 — the lookahead analysis was over-claimed.**
  - Finding: strong LL(k) is weaker than LL(k) for k > 1. Eager FIRST_k and
    FOLLOW_k grow as |T|^k, so "linear" was false. Soft keywords by FOLLOW
    viability accept `CREATE TABLE select(…)`, which SQLite rejects. The
    10,000,000-step compile-time limit was not addressed.
  - Repair (§3.2, §6, G-A9, G-A16, G-D11):
    - named SLL(k);
    - lazy deepening only at LL(1) conflicts, with a cap and
      `Jet.GrammarTooComplex`;
    - explicit fallback plus a reserved-word check from the plan;
    - recorded step counts.
- **R-H4 — the operand of `BETWEEN x AND y` was unspecified.**
  - Repair (§3.1): the middle-operand rule.
- **R-H5 — `print` could inject.**
  - Finding: a hand-built value with a token field holding grammar syntax
    printed new structure. Paren insertion assumed a grouping form it could
    not find.
  - Repair (§4): `print` is fallible and checks token fields; parentheses go
    through the grouping alternative or give an error.
- **R-H6 — Sh positions the draft did not cover.**
  - Finding: a `String` hole in the program position picks the executable.
    A bare newline was whitespace, which merges what a shell runs as two
    commands. `$` and backtick were allowed inside double quotes.
  - Repair (§8.6, G-D22b, ballot B7).
- **R-H7 — proofs and paths that cannot run as written:**
  - `--filter audit`, `spec` and `reference` select nothing meaningful;
    `db_policy` selects no row (the directory is `DBPolicy`);
  - `jet self devtools grammars --check` does not exist; the tool rewrites
    files and ignores the flag;
  - codes were to be registered in the generated `DiagnosticRows.jet`
    instead of `Compiler/Prelude/Diagnostics.jet`, and all at once, which
    `--check` refuses for unreported codes;
  - `Tests/Cases/Grammar/` is already Jet's own grammar;
  - Regex lives in `Core/Regex/Regex.jet`, not `Core/Text/Regex.jet`;
  - G-B17 named "JIT, interpreter" tiers that do not exist (I9);
  - `core grammar` narrowing was undefined.
  - Repair (§15 conventions and every proof cell): each proof checked against
    `examples.md`, `cli.md`, `Exec.jet`, `Front.jet` and the case tree.
- **R-H8 — items over one hour.**
  - Finding: G-B3 (about 45 files), G-B10, the six SQL grammar items, and the
    six engine-sink items covering 405 rules were each well over an hour. G-D7
    had no corpus. G-E4, E5 and E8 were several items each.
  - Repair: G-B3a–g, G-B10a/b, G-D1–D12 with a corpus item, G-D18.1–16 with
    per-family differential tests, and explicit ×N rows.
- **R-H9 — generated types would join Core's public surface.**
  - Repair (§2.2 item 5): items take the binding's visibility, Core head
    grammars are `priv`, and `CoreSurfaceLedger.jet --check` is in G-A1.

**Medium**

- **R-M1:** the draft gave `encode_hole` a `Hole` return in §1. The Prelude
  declares `String`; corrected.
- **R-M2:** collect-all through `graph.keep_going` (§2.4).
- **R-M3:** no SQL corpus exists in the repository; G-D11 builds one.
- **R-M4:** the spec.md:97 `^` defect was confirmed and is fixed in G-C2.
- **R-M5:** the `Shape` embedding route (`BodyExpression`) is now named.
- **R-M6:** keyword lookup by perfect hash; the SWAR claim was softened.
- **R-M7:** URL `Host` and `Userinfo` slots defined.
- **R-M8:** SQL list holes bind their display text today and are refused.
- **R-M9:** a new Core module needs a layer and `Core.rows` rows; the layer
  rule forbids `core.grammar → core.text.html` (G-A1).

**Low**

- **R-L1:** the §1 facts were re-read at `e1d1b579653`. HTML line ranges were
  corrected, and the Regex line range now runs to 2393.

### Verdicts on the draft's claimed strengths

| Claim | Verdict |
|---|---|
| No existing mechanism turns a literal into types; the declaring head is the smallest addition, not a second mechanism (I8) | **Holds within stated limits.** Root `#loop` + `#run` is a real existing route the draft dismissed too quickly, but it cannot loop fields or emit recursive code, and it costs a probe compile. As drafted, the spelling list and the meaning held in the compiler failed; repaired by R-H1 |
| Module-binding registration can wait on a compile-time value | **Fails as drafted:** only through the whole-program probe compile. **Repaired** by the pre-registration batch, which is unproven until G-B8a |
| The CheckedText slot change fits D-TEXTHEAD-HOLES1 and its implementers | **Fails as drafted** (R-C2). **Holds within stated limits** after the repair and the owner's B3 approval, which amends D-TEXTHEAD-HOLES1 |
| LL(k) is the right class for SQLite's grammar and is fast | **Holds within stated limits:** SLL(3) with explicit fallback, decided by G-D12. Speed is unmeasured and gated by the pairs |
| Grammar analysis cost is linear and fits the compile budget | **Fails as drafted** (eager FIRST_k; probe pass). **Holds within stated limits** after R-C1/R-H3, pending G-A16 and G-P1 |
| Left-recursive or ambiguous grammars fail the build with rules and a sample | **Holds within stated limits:** an LL conflict is a superset of ambiguity (stated), and the operator form of direct left recursion is now allowed, as in ANTLR |
| One meaning on every tier (I9) | **Holds** for the mechanism (the check runs in the child; parse is ordinary generated Jet). G-B17's tier list failed and was repaired |
| SQL holes typed and escaped by position | **Fails as drafted** (R-C4). **Holds** after the repair |
| HTML escapes by position | **Fails as drafted** (R-C3). **Holds within stated limits** after the repair; the script and style refusal is decided (§14) |
| Sh: each hole is one argv item | **Holds.** Program choice and newline handling are decided by B7 (§14) |
| URL, DateTime, Path and Regex move onto grammars | **Holds within stated limits:** `DateTime` fits the compile-time value algebra (verified). Whether the Regex program's sized-number lists and `priv` fields embed is unverified (G-D30), and there is a run-time fallback |
| One SQL grammar replaces three readers | **Holds within stated limits:** gated by `sql_engine`, `sql_policy`, the differential tests and G-P2 |
| Every item is at most one hour, with exact files and a runnable proof | **Fails as drafted** (R-H7, R-H8). **Holds** after repair, except G-C4's highlight colors, which no command proves |
| Peer comparison | **Holds within stated limits:** Jai is unverified; ANTLR and Bison were added |

### Unrepaired

- B7 and B8 are decided as their safe options under the owner's P31
  approval (§14). G-D22b and G-D20b are unblocked.
- The feasibility of pre-registration staging (G-B8a) is unproven.
- The Core bodies record is ruled not a compile cache (§14).
- `TypeId` per hole, compile-time representability, recursive structs and
  the Core.rows treatment of generated items are open [INFERENCE] items, each
  with the item that settles it.
