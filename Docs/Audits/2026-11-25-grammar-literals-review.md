# Grammar literals design review: behavior → source → proof (2026-11-25)

This is an adversarial review of the grammar-literal design and build plan for
card #7036. It checks each claim against the source at dev `e1d1b579653`,
and the plan is repaired in place. Each row gives a behavior the plan relies
on, the source that shows today's state, and the proof that will hold the
behavior once built. A proof marked *planned* names an item of the plan; that
proof does not exist yet.

Decisions involved:

- D-TEXTHEAD-TYPE1=A: one pure `check` for literals and `Type.from`.
- D-TEXTHEAD-HOLES1=A: `check` sees `{}`-joined text; no literal fails at run
  time.
- D-META-LADDER1: types from text need Generate, Types and Raw.
- D-META-CODE1: items are built as trees, never as text.
- D-GENERIC-TYPEFN1: archived on 2026-10-29.
- The owner's P31 approval (2026-11-25) of the declaring form, the standard
  notation, the CheckedText change, the Core names, and the Sh and HTML
  refusals.

## 1. Mechanism

| # | Behavior | Source | Proof |
|---|---|---|---|
| 1.1 | A checked head's `check` runs as Core code in the compile-time child; only pass or fail returns | `Compiler/JetSema/Source/Sema/Expressions/Constructors.jet:2023-2177`; `ComptimeFunctions.jet:32` | `Tests/Cases/CheckedText/checked_text_hooks_check_encode_and_build.case` (exists) |
| 1.2 | No existing mechanism makes nominal types with computed fields and recursive code from a literal in library code. Marker expand allows a fixed set of methods; declared blocks are statements; `meta.add`/`b.generate` are root-only; root `#loop` cannot loop fields | `Sema/Registration/Derives.jet:421`; `UserTemplates.jet:901-914`; `Docs/Spec/spec.md:7563-7597` | planned: `Tests/Cases/GrammarLiterals/types_are_generated.case` (G-B9) |
| 1.3 | A root `#loop` waiting on a `#run` value costs a whole probe compile | `Compiler/JetDriver/Source/Driver/Pipeline.jet:~5959-5971`; `Sema/Registration/Items.jet:733-741` | `Tests/Cases/ComptimeInputs/comptime_root_loop_waits_on_run_value.case` (exists); planned: Throughput gate G-P1 |
| 1.4 | The declaring head is found by `use`-table identity, not by spelling. Today's boundary heads are a spelling list | `Compiler/JetFoundation/Source/Facts/Checked.jet:134`; `Loader.jet:4589` | planned: `Tests/Cases/GrammarLiterals/declaring_head_is_found_by_identity.case` (G-B7) |
| 1.5 | Core derive expansions are recorded in the Core bodies file; Core grammar plans follow that pattern | `Sema/CoreBodies.jet:960-1052` | planned: `Tests/Cases/GrammarLiterals/core_grammar_plan_comes_from_the_core_record.case` (G-B8c) |
| 1.6 | `Type.from` and the run-time check use the same `check(text)` (E0907). The amended trait must keep one pure check | `Constructors.jet:1862-2021` | `Tests/Cases/CheckedText/dynamic_checked_text_from_keeps_the_shared_error_report.case` (exists) |
| 1.7 | A rejected literal stops the build; the reason is sniffed from fields | `Pipeline.jet:3338-3349, ~3378` | planned: `Tests/UI/checked_text_reports_every_literal.jet` (G-B6) |

## 2. Heads (I1)

| # | Behavior | Source | Proof |
|---|---|---|---|
| 2.1 | Today SQL accepts any text, and a list hole binds its display text | `Core/DB/DB.jet:325-346` | planned: `Tests/Cases/GrammarLiterals/sql_head_holes_typed_by_position.case` (G-D14) |
| 2.2 | SQL expression holes need one `Operand` slot chosen by hole type. Separate `Value` and `Name` slots collide | design §8.4 | planned: G-D14 |
| 2.3 | HTML picks the escaping with a run-time state machine. It has no rule for `on*`, `style`, `srcdoc` or `srcset`, and passes a `URL` hole as is | `Core/Text/HTML.jet:115-205` | planned: `Tests/Cases/GrammarLiterals/html_head_escapes_by_position.case` (G-D21); ballot B8 |
| 2.4 | Sh splits on whitespace (newline included), so `a{x}b` gives three items; a `String` may be the program | `Core/Process/Process.jet:142-164` | `Tests/Cases/CoreLibPartsPlatform/core_process_sh_typed_text_keeps_each_hole_one_argv_item.case` (exists); planned: G-D23; ballot B7 |
| 2.5 | URL parses again through a nonce marker | `Core/Net/URL.jet:171-298` | planned: G-D24b |
| 2.6 | DateTime parses again in `build` | `Core/Time/Time.jet:1182-1199` | planned: G-D26 |
| 2.7 | Regex runs `core.regex` in the compiler process | `Constructors.jet:2340-2393`; `Core/Regex/Regex.jet:706-1066` | `Tests/Cases/DSLBlocks/regex_typed_head_owns_backslashes_but_rejects_holes.case` (exists); planned: G-D30 |
| 2.8 | spec.md:97 says `^` where the code escapes `&` | `Docs/Spec/spec.md:97`; `Core/Text/HTML.jet:186-189` | planned: G-C2 |
| 2.9 | HTML attribute classes and the URL scheme rule follow Go's `html/template`: `attrTypeMap` and `attrType` (the `data-` and `prefix:` stripping, `on*` as script, and the `src`/`uri`/`url` heuristic), and `isSafeURL` (`http`, `https` and `mailto` only, when a `:` comes before any `/`) | Go `src/html/template/attr.go`, `url.go` (read 2026-11-25) | planned: G-D20b, G-D21 |
| 2.10 | The SQL fallback keywords already exist as data in the Lemon port, so the grammar's `fallback_keyword` rule is checked against them, not copied | `Core/DB/Engine/ParseTables.jet:1-14, 6013` | planned: `Core/DB/Tests/SQLGrammar.jet` (G-D1) |

## 3. Plan mechanics

| # | Behavior | Source | Proof |
|---|---|---|---|
| 3.1 | Test filters match repository-relative row IDs; a filter that matches nothing fails. `audit`, `spec`, `reference` and `db_policy` are invalid filters | `Docs/Spec/Reference/cli.md:1270-1298`; `Compiler/JetTestKit/Source/Exec.jet:53-67` | every §15 proof cell rewritten |
| 3.2 | `jet self devtools grammars` has no `--check`; a case is the guard | `Compiler/JetCLI/Source/CLI/Front.jet:3370`; `EditorGrammars.jet:118` | `Tests/Cases/Grammar/editor_grammars_match_generated_sections.case` (exists) |
| 3.3 | Diagnostics are declared in `Compiler/Prelude/Diagnostics.jet`. The generated rows refuse unreported codes, so each code lands with its reporter | `Tools/Agent/DiagnosticRows.jet:1-38` | `jet Tools/Agent/DiagnosticRows.jet --check` |
| 3.4 | `Tests/Cases/Grammar/` is Jet's own grammar; new cases go under `Tests/Cases/GrammarLiterals/` | `Tests/Cases/Grammar/` | — |
| 3.5 | A new Core module needs a layer and `Core.rows` rows | `Core/package.jet:7-12`; `Compiler/Prelude/Core.rows:196-197` | `jet Tools/Agent/CoreSurfaceLedger.jet --check` (G-A1) |
| 3.6 | The compile-throughput gate exists and judges CPU and instructions against a seed | `Tools/Perf/CompileThroughput/Throughput.jet:71-94` | planned: G-P1, G-P2 |
| 3.6a | A compile-time value is drawn from an algebra (`Int`, `Float`, `Bool`, `Char`, `String`, enums, lists, structs; no maps or handles). `GrammarPlan` is designed inside it. `DateTime` fits it | `Docs/Spec/Reference/metaprogramming.md:86-94, 976-983`; `Core/Time/Time.jet:979-982` | planned: `Core/Grammar/Tests/Plan.jet` (G-A13) |
| 3.6b | `Examples/Text/CheckedText/` holds seven files that implement or call the trait, not the four the draft listed | `Examples/Text/CheckedText/` | planned: `jet test Compiler --profile golden Examples/Text/CheckedText` (G-B3c) |
| 3.7 | The compile-time step limit is 10,000,000 per call | `Docs/Spec/Reference/metaprogramming.md:180-220` | planned: `Core/Grammar/Tests/Budget.jet` (G-A16); G-D11 |

## 4. Notation (owner direction: the industry standard)

The rule form is `name : alternatives ;`, the form of ANTLR 4 and of
yacc/Bison. Lark uses the same form without the `;`.

Taken from the peers:

- **ANTLR 4:** labels `x=`/`x+=`, `# Alt`, `fragment`, `-> skip`, `import`,
  `EOF`, and operator precedence by direct left recursion.
- **W3C:** both quote styles and `[…]`/`[^…]` classes.
- **Lark:** `"x"i`.

The one Jet addition is `HOLE<slot=Name>`, in ANTLR's element-option syntax.

Sources:

- ANTLR `doc/parser-rules.md`, `lexer-rules.md` and `left-recursion.md`;
- the GNU Bison manual, "Syntax of Grammar Rules";
- the Lark grammar reference;
- W3C XML 1.0 §6;
- the pest book, "Syntax".

## 5. Peers, fairly

| Peer | Strongest point | Where Jet differs | Source |
|---|---|---|---|
| ANTLR 4 | ALL(*) parses any non-left-recursive CFG; typed contexts per rule and label | Jet makes conflicts build errors instead of resolving them by order. Jet's types are nominal and checked at build time | antlr.org ALL(*) report; antlr4 docs |
| Bison | LALR(1) and GLR; mature precedence declarations | No types come from the grammar; semantic values are written by hand | GNU Bison manual |
| lark | Earley handles any CFG; contextual lexer | Jet has a typed tree and no run-time grammar | lark docs |
| pest | PEG with clear left-recursion errors | Ordered choice hides ambiguity; pest has a generic `Pair` tree | pest book |
| Raku | Grammars in the language, action classes | Backtracking; untyped match objects | docs.raku.org grammars |
| tree-sitter | Incremental GLR, error recovery | A concrete syntax tree only | tree-sitter docs |
| nom | Zero-copy, streaming, the author's own types | Code, not a grammar; no ambiguity check | docs.rs nom |
| Hand-written Core parsers (Lemon SQL, JSON, XML, HTTP) | Tuned for one format | Each deletion is gated by a paired benchmark | §10 of the design |

## 6. Findings by severity

- **Critical:**
  - R-C1: staging needed a second whole compile.
  - R-C2: the CheckedText change broke `from`, `raw` and the run-time path,
    and its cutover list was incomplete.
  - R-C3: HTML holes in script, style and URL contexts.
  - R-C4: SQL slot collision.
- **High:**
  - R-H1: a spelling list, meaning in the compiler, decisions not faced.
  - R-H2: an invented notation.
  - R-H3: an over-claimed LL(k) analysis.
  - R-H4: the `BETWEEN` operand.
  - R-H5: print injection.
  - R-H6: Sh program and newline.
  - R-H7: invalid proofs and paths.
  - R-H8: oversized items.
  - R-H9: generated types becoming public surface.
- **Medium:** R-M1–R-M9.
- **Low:** R-L1.

Each finding's repair is in §17 of the design and in the repaired sections.

## 7. Verdicts

| Claim | Verdict |
|---|---|
| The smallest addition, not a second mechanism (I8) | Holds within stated limits after R-H1 |
| Registration can wait on a compile-time value | Fails as drafted; repaired, unproven until G-B8a |
| The CheckedText slot change | Fails as drafted; holds within stated limits after R-C2 |
| LL(k) fits SQLite and is fast | Holds within stated limits (SLL(3); G-D12; pairs) |
| Analysis cost fits the compile budget | Fails as drafted; holds within stated limits pending G-A16 and G-P1 |
| Build errors for left recursion and ambiguity | Holds within stated limits |
| One meaning on every tier (I9) | Holds; the tier list was repaired |
| SQL holes typed by position | Fails as drafted; holds after R-C4 |
| HTML escaping by position | Fails as drafted; holds within stated limits after R-C3, with B8 decided as A |
| Sh: one hole, one argv item | Holds; B7 decided as A |
| URL, DateTime, Path and Regex on grammars | Holds within stated limits |
| One SQL grammar replaces three readers | Holds within stated limits |
| Items at most one hour, with runnable proofs | Fails as drafted; holds after repair (except G-C4's colors) |
| Peer comparison | Holds within stated limits (Jai unverified) |

## Open

- B7 (D-SH-ARGV1) and B8 (D-HTML-SCRIPTATTR1) were decided as their safe options (A) under the owner's P31 approval, and the Core bodies record was ruled not a compile cache; see the design's §14.
- [INFERENCE] items, each settled by a named plan item:
  - pre-registration staging (G-B8a);
  - one `TypeId` per hole in the CheckedText request (G-B4);
  - whether the Regex program's `[I64]`/`[U8]` lists and `priv` fields embed
    as compile-time values (G-D30; there is a run-time fallback);
  - whether a struct can hold itself (G-A11; there is a single-case-enum
    fallback);
  - Core.rows handling of generated `priv` items (G-B8c);
  - SQL in SLL(3) (G-D12);
  - SWAR (only if a pair needs it).

## Run

Nothing was built, tested or measured. The review read the source, the spec,
Tower decisions (read-only) and the peers' public documentation. It also read
these external sources: ANTLR 4's `doc/*.md`, the grammars-v4
`SQLiteParser.g4`, Go's `html/template/attr.go` and `url.go`, the Bison
manual, the Lark reference, the pest book and W3C XML §6. One read-only
count (§10 of the design: 315 parser-named functions in 77 Core files) ran
as an inline `node -e` with no files written. Every *planned* proof above is
unrun.
