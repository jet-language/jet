# Comment sigil: `##` instead of `//`?

Date: 2026-11-04. Author: Research3 (Claude). Status: research note behind
ballot D-COMMENT-SIGIL1 on card #6566. The card's acceptance criteria are the
eight defects in §10.

Owner question: *"Consider ## for comments rather than // -> Frees the more
accessible / and # as a metaprogramming mark kinda fits thematically as a meta
meta line (comment) -> Research & thoughts?"*

Repo: `dev` at `08dde966`. Probes ran on the admitted compiler
`$JET_SCRATCH_ROOT/discovery/base/latest/jet`, commit
`1f70b8d4bd91cb88f4eda995388d67e288aac8df`, generation s1. Every probe output in
`probes/out.txt` starts with a header that names this commit.

Assets: probes, outputs and census scripts are in
`Docs/research/assets/2026-11-04-comment-sigil/`. Below, `probes/…` and the
`*.mjs` names are relative to that directory. The census scripts take the
repository root as their argument. `probes/run.mjs` needs `JET_SCRATCH_ROOT`
and `JET_TOOLCHAIN_ROOT`.

## Answer in brief

Keep `//`. The idea is not foolish: `##` has real precedent (Nim's doc
comments, Doxygen for Python), it is one key press on UK and German keyboards,
and Python, shell and Nix readers are used to `#` comments. But both halves of
the motivation fall apart once you check:

1. **Moving comments frees nothing anyone needs.** The single `/` is never a
   comment, so it is already free for division and the positional-only
   parameter zone. Only the doubled `//` and `/*` are taken, and their one
   strong rival use is Python's floor division. Jet ratified that as `/%` on
   2026-08-05, and it is used 1,057 times in code. Regex is a typed literal
   (`Regex{"..."}`) and paths are typed values. No ratified or proposed Tower
   decision needs `//` or a slash literal.
2. **`##` cuts against the ratified meaning of `#`.** D-META-HASH1 A (ratified
   2026-10-30) sold `#` to the owner as "the compiler acts here": markers, site
   values and `#{x}` pastes. A comment is the one place where the compiler
   does nothing at all. D-PRE-COMPILE1 (ratified A, 2026-10-31) also already
   rejected a doubled-hash form, `##Pre`. Every language that gives `#` to
   compiler work keeps `//` for comments: C, C++, C#, Swift, Rust, Odin and
   Jai. Jai is the direct ancestor of Jet's `#` vocabulary.

Keeping `//` has one real cost that the probes surfaced, and it should be
fixed whatever the owner decides about `##`. A Python habit, `half :: 7 // 2`,
compiles today and silently binds 7 (probe c11). The ballot's option A adds a
warning for exactly that shape.

## 1. Current state

### Forms

| Form | Spelling | Where it is law |
|---|---|---|
| Line comment | `// text` | S5; `COMMENT_PREFIX :: "//"` at `Compiler/JetFoundation/Data/Syntax/CoreSurface.jet:189` |
| Block comment, nesting | `/* … */` | S5 (ratified 2026-06-15); `BLOCK_COMMENT_OPEN/CLOSE` at `CoreSurface.jet:192-195`; unclosed is E0002 |
| Doc comment | `/// text` | S49 (`Docs/spec/syntax-decisions.md:3963-3967`): plain text, doctest-runnable, no `/** */` |
| Module doc | the first `///` block at the top of a file | `jet doc` |
| `//!` | used 53 times (Examples 39, tests 14), e.g. `Examples/features/basics/digits.jet:1-3` | **Not a ratified form.** S49 names only `///`. It lexes as a plain comment. |
| Shebang | `#!/…` at byte 0 | `Scan.jet:682-685`; anywhere else is E0042 (probe c04) |

### Lexer (`Compiler/JetLexer/Source/Lexer/Scan.jet`)

- `716-726`: `//` produces `TOKEN_LINE_COMMENT` (55). The one exception: on a
  *config surface* (`lex_config`, used by manifests, `ForeignDeps.jet:23`, the
  `ModuleEval` env files and `ConfigProgram.jet:18`), a `//` right after `:`
  lexes as two `/` tokens so that `scheme://host` survives. This special case
  exists only because `//` starts comments.
- `727-731` and `633-653`: nested `/* */` produce `TOKEN_BLOCK_COMMENT` (56);
  unclosed gives E0002 (probe c07).
- `818-822`: `#` is always a single `TOKEN_HASH`. So `##` today is two hash
  tokens and is an error in every position (probes c02 and c12: "Expected a
  name after `#`"). **`##` is free.**
- There is no doc-comment token. `///` is recognized by text prefix in each tool.

### Registry bypass

No Jet source reads `COMMENT_PREFIX`, `BLOCK_COMMENT_OPEN` or
`BLOCK_COMMENT_CLOSE`; only the registry file and an old audit TSV mention
them. The lexer and about 30 tool scanners spell `"//"` and `"///"`
themselves (list in §6). This breaks the AGENTS.md rule that syntax spellings
come from the registry, and it is listed as a defect below.

### What `#` does today, and what is ratified next

| Job | Example | Source |
|---|---|---|
| Marker or rule | `#Inline`, `#[Test, Memo]`, `#Unsafe("why") { … }` | `RULE_PREFIX :: "#"`, `Markers.jet:4-15` |
| Negative marker | `#!Printable` | markers registry |
| In-body setting | `#Policy :: …` | `Markers.jet:17-24`; all `#` jobs are told apart by parser position only (`Markers.jet:10`) |
| Fixed-size list | `[Int#3]` | probe c05 |
| Version pin, channel | `pkg#1.2.0`, `#latest`, `#auto` | Jetpack |
| Site values, pastes | `#line`, `#file`, `#package`, `#{x}` | D-META-HASH1 A, ratified 2026-10-30, **not built yet** (probe c05b: `#line` is E0927 today) |
| Shebang | `#!/usr/bin/env jet` | lexer special case |

D-META-HASH1's one-sentence meaning is "`#` means the compiler acts here".

## 2. Census

Script: `census.mjs` (output `probes/census.json`). It covers every tracked
`.jet` file under `Compiler/`, `Core/`, `Examples/`, `tests/` and `Jetpack/`:
17,255 files and about 1.27M lines.

| Measure | Count |
|---|---|
| `//` comment tokens | **94,746** |
| — full-line | 90,855 |
| — trailing after code | 1,140 |
| — `///` doc lines | 2,698 |
| — `//!` lines (unofficial) | 53 |
| `/* */` block comments | 17 (3 nested, all in formatter tests) |
| `////` lines | 0 |
| Files containing a comment | 7,815 |
| Comments whose body contains `#` | 5,637 |
| Comment bodies that *start* with `#` (mostly card refs such as `// #4828:`) | 829 |
| Commented-out markers (e.g. `PackageFiles.jet:435 // #[Rename("wire_key")]`, `RuntimeRoutes.jet:2 // #Export(c)`) | 28 |
| Comments containing `##` | 2 |
| Comments containing `#{` | 1 (`Compiler/JetSema/…/Effects/Checks.jet:1243`) |
| Markdown headings inside doc comments | 0 |
| `#` tokens in code | 14,325 |
| `##` in code | 0 |
| `/` in code | 2,078 |
| `/%` in code | 1,057 |
| `://` in code outside strings | 0 |

Per directory (line comments / doc lines): Compiler 56,262 / 1,843; Core
4,828 / 771; Examples 5,965 / 14; Jetpack 16,836 / 0; tests 11,014 / 70.

Docs (`docs-census.mjs`, output `probes/docs-census.json`): 766 tracked `.md`
files hold 607 ```` ```jet ```` fences. Inside them are 376 comment lines, 3
doc lines and 251 trailing comments, across 40 files.

Comments that tools read as directives (`directive-census.mjs`, output
`probes/directive-census.json`):

| Directive | Lines | Reader |
|---|---|---|
| `// runtime-init:` / `runtime-fini:` | 52 | runtime generators |
| `// jet-ffi-descriptor=` | 50 | FFI binding loader |
| `// =>` doctest expectation | 40 | `JetCli/Source/Cli/Docs.jet:24` |
| `// jet:line N` | 18 | `JetDebug/Source/LineMap.jet:43` |
| `// canvas:comment`, `// canvas:collapse` | 7 | devserver canvas |
| `// generated by:` | 2 | `JetCli/Source/Cli/Bindings.jet:30` |

These machine-read comments are the most "meta" thing that comments do today.
A later decision could move them to real `#` forms; that, and not the comment
sigil, is where the owner's "meta" instinct pays off (§7).

## 3. What `/` would actually free

| Candidate | Blocked by `//` today? | Status |
|---|---|---|
| Single `/` division, `/=` | No | 2,078 and 10 uses |
| Positional-only parameter zone `fn f(a, /)` | No | works (probe c08) |
| Python floor division `a // b` | Yes | **Already ratified as `/%`** (registry `MathLayout.jet`, "ratified 2026-08-05"), 1,057 uses. A second spelling would break "one mechanism". |
| Nix-style attribute merge `a // b` | Yes | No proposal in Tower. |
| Regex literals `/re/` | Partly (`/*` and `//` edge cases) | Regex is the typed literal `Regex{"..."}`, checked at build time (`Core/Regex/Regex.jet:14`, D-SCRIPT-REGEXLINT1). |
| Unquoted path literals `/etc/x`, `./a/b` | Partly | Paths are typed native values (D-OSPATH-NATIVE1). An unquoted path also collides with division, whatever the comment sigil. |
| Unquoted URLs in config | Needs the `://` special case | Works today via the `Scan.jet:717-722` exception. |

A Tower search found no ratified, open or proposed decision that needs `//`,
`/*`, slash regex or slash paths. **What would be freed is the doubled glyph,
and its only natural meaning (floor division) already has a ratified
spelling.** The single, "more accessible" `/` is not occupied by comments.

### The one real problem `//` causes

Probe c11:

```jet
fn run() {
    half :: 7 // 2
    print(half)
}
```

prints `7`, exit 0. A Python user's floor division silently becomes "7, plus a
comment". Python is the most common first language, so this is a real
beginner trap.

**The warning has to be typed, not lexical.** `floor-warn-census.mjs` (output
`probes/floor-warn-census.json`) tried lexical rules on the corpus:

| Rule | Lines it would flag |
|---|---|
| `//` after an operand with at most one space, body starts with a name or number | 367 |
| …and the whole comment body is an operand expression | 146 |
| …and after `)` or `]` the body must start with a digit | 61 |

All 61 are label comments, not division. Examples are `return false // WB3`,
`i = i + 1 // SB3`, `C_WANT_READ :: -26880 // MBEDTLS_ERR_SSL_WANT_READ -0x6900`
and `scan_test_f32(…) // 1.0`. The single-space style is common in
unformatted files, so formatter spacing cannot be relied on (the formatter
writes two spaces, probes c01 and c14). The rule that works is semantic. The
code before `//` on the line ends a number-typed expression. The comment text
parses as a number expression. Every name in it resolves in the current
scope. Each of the 61 lexical hits fails that rule: it ends in a Bool or a
unit call, or it names something that is not in scope (`WB3`, `SB3`,
`MBEDTLS_…`) [INFERENCE: checked by reading the 61 lines, not by running a
checker]. The warning text: "This `//` starts a comment, so the line keeps
only `7`. Whole-number division is `/%`: `7 /% 2`." The ballot's option A
includes it.

Option B removes this trap a different way (`//` becomes a teaching error),
but trades it for a symmetric one: a doubled `#` before a marker. See §5.

## 4. Other languages

### Who uses what

| Language | Line | Doc | Block | What `#` means |
|---|---|---|---|---|
| C / C++ | `//` | (Doxygen `///`) | `/* */`, not nested | preprocessor; **`##` pastes tokens** |
| C# | `//` | `///` | `/* */` | `#if`, `#region` directives |
| Swift | `//` | `///` | `/* */`, nested | `#if`, `#file`, `#line`, macros `#stringify` |
| Rust | `//` | `///`, `//!` | `/* */`, nested | `#[attr]`, `#![inner]`. The lexer must tell a `#!` shebang from `#![`. |
| Zig | `//` | `///`, `//!` | none | not used; builtins are `@` |
| Odin | `//` | `//` above | `/* */`, nested (odin-lang.org/docs/overview) | directives `#force_inline`, `#partial`, `#load` |
| Jai | `//` | `//` above | `/* */`, nested | directives `#run`, `#insert`, `#caller_location`, `#code` |
| Gleam | `//` | `///`, `////` (module) | none | — |
| Python | `#` | docstrings | none | comment only |
| Ruby | `#` | `#` (YARD/RDoc) | `=begin/=end` | comment (and `#{}` interpolation inside strings) |
| Perl | `#` | POD | POD | comment |
| Shell, YAML, TOML, R | `#` | R roxygen `#'` | none | comment |
| Elixir | `#` | `@doc` | none | comment |
| Julia | `#` | docstrings | `#= =#`, nested | comment |
| Nim | `#` | **`##`** | `#[ ]#`, `##[ ]##`, nested (nim-lang.org manual) | comment; pragmas are `{. .}` |
| Nix | `#` | — | `/* */` | comment; **`//` is the attribute-set merge operator** |
| CoffeeScript | `#` | — | `###` | comment |
| Lua | `--` | (LDoc `---`) | `--[[ ]]` | `#` is the length operator |
| Haskell, SQL, Ada, Elm | `--` | Haddock `-- \|` | `{- -}` (Haskell) | — |
| Lisp family | `;`, `;;`, `;;;` by convention | docstrings | `#\| \|#` | reader macros |

Patterns:

- **No language that uses `#` for compiler work also uses `#` for comments.**
  C, C#, Swift, Rust, Odin and Jai all pair `#` meta with `//` comments. Jet's
  ratified `#` vocabulary (D-JAI-SITE1, D-JAI-CODE1, D-META-HASH1) follows Jai
  most closely.
- `##` as a comment opener appears only as Nim's doc comment (where `#` is the
  comment) and as Doxygen's Python doc block. Nowhere is `##` the *plain*
  comment.
- `##` already has a meta meaning to C programmers: **token pasting**
  (`a ## b`). Jet's paste is `#{x}`. So the closest precedent for "doubled
  `#`" means "paste", not "comment".
- Nix is the one language that freed `//` on purpose: its comments are `#`
  and `//` is merge. Nix does not use `#` for anything else.

### Keyboard reach

| Layout | `/` | `#` |
|---|---|---|
| US, US-International | unshifted | Shift+3 |
| UK | unshifted | unshifted (key by Enter) |
| German | Shift+7 | unshifted (key by Enter) |
| Swiss German | Shift+7 | AltGr+3 |
| French AZERTY | Shift+`:` key | AltGr+3 |
| Spanish | Shift+7 | AltGr+3 |
| Italian | Shift+7 | AltGr+à |
| Nordic (SE, NO, DK, FI) | Shift+7 | Shift+3 |
| Portuguese | Shift+7 | Shift+3 |
| Japanese JIS | unshifted | Shift+3 |

Each glyph is unshifted on about three layouts and shifted elsewhere. `/` wins
on US and JIS; `#` wins on German; UK ties. Only French AZERTY makes `#`
clearly harder (AltGr) than `/` (Shift). It is a wash. Commenting out code
uses the editor's toggle-comment command whichever glyph Jet picks [INFERENCE:
VS Code binds it to the key at the US `/` position, shown as `Ctrl+#` on
German layouts].

### Readability

- A dense run of `#` marks at line start is harder to scan than `//`. Jet code
  starts lines with markers (`#Test(…)`, `#Inline`, `#Unsafe(…) {`), and 829
  comments already start with `#4828`-style card refs. Under `##` these become
  `## #4828: …` beside `#Test("…")`. Comments and markers stop being visually
  distinct without colour, which matters in diffs, terminals, review tools and
  GitHub. Linguist has no `.jet` entry (checked `languages.yml`, 2026-11-04),
  so GitHub shows Jet as plain text.
- `//` reads as "not code" to readers of the C family, the larger group by
  far, and to everyone who reads Jet's peer languages.

## 5. Thematic fit and edge cases

**Does `##` read as meta-meta?** In the linguistic sense, a comment is text
about code, so the thought has some truth. But Jet's ratified `#` is narrower:
"the compiler acts here" (D-META-HASH1). A comment is the one line where the
compiler does nothing. `##` would be the only `#` form with the opposite
meaning. D-PRE-COMPILE1 also already turned down a second doubled-hash mark
(`##Pre`). Doc comments *are* read by tools (`jet doc`, doctests, hover), and
there is a `#Doc("…")` marker for CLI help (`Markers.jet:94`). That is an
argument for the doc form being `#`-shaped, not the plain comment.

| Case | Today (`//`) | Under `##` |
|---|---|---|
| Commented-out marker | `// #Inline` | `## #Inline`. Fine with a space. |
| Doubled-hash typo | `##Pre(x > 0)` is an error (probe c12) | Becomes a **silent comment**: the contract disappears. Needs a rule that `##` must be followed by a space (B's design). |
| Python floor division | `7 // 2` silently binds 7 (probe c11) | `//` becomes a teaching error. Gain for B. |
| `#{x}` paste inside a comment | text (probe c13) | text, same |
| `#line` inside a comment | text | text, same |
| Shebang | `#!` at byte 0 only; E0042 elsewhere (c04) | Same rule. No gain: `##` comments do not make `#!` "just a comment", because `#!Name` is the negative marker. |
| Doc comment form | `///` | `###` (mirrors `///`; CoffeeScript uses `###` for block comments) or `##!`. `##!` collides visually with `#!Name` negative markers and shebangs. `###` is the only clean choice. |
| Markdown heading in a doc | `/// ## Examples` | `### ## Examples`. Legal, but a doc line *is* a markdown heading glyph. Corpus has 0 today. |
| Module doc | first `///` block | first `###` block |
| Divider lines | `////…` plain comment (0 uses) | `####…` plain comment |
| Block comment | nested `/* */` (17 uses) | Nim's `#[ ]#` collides with `#[A, B]` marker lists. Julia's `#= =#` would work. Simplest is to drop block comments, as Python and Zig do. Keeping `/* */` would leave `/` still starting comments. |
| Config URLs | `://` special case | special case deleted |
| `[Int#3]`, `pkg#1.2.0` | unaffected | unaffected (not at line start) |

### Tooling

- **Formatter:** `Compiler/JetFormatter/…/Formatter.jet:815`,
  `Statements.jet:284-295, 1269`, `Items.jet:1657` spell `//` themselves.
- **tree-sitter:** `Tools/editors/tree-sitter/grammar.js:521-523` (`line_comment`,
  `doc_comment` at prec 1, `block_comment`). The generated `src/parser.c` (128k
  lines) and the wasm must be regenerated. Its `block_comment` regex does not
  nest (defect below).
- **VS Code:** `language-configuration.json` sets `lineComment: "//"` and no
  `blockComment`. `syntaxes/jet.tmLanguage.json:69-83` matches `///.*$` and
  `//.*$`, and has **no block-comment rule** (defect below). Line 65 embeds
  `//` and `/* */` in the `package` lookahead. `Tools/editors/jet.tmGrammar` is
  a copy.
- **Zed:** `zed/languages/jet/config.toml:4-5` (`line_comments = ["// ", "/// "]`),
  `highlights.scm:11-14`.
- **GitHub:** no Linguist entry, so there is nothing to change there.

## 6. Cost of switching to `##` (one greenfield cutover)

- **Source:** 94,746 comment tokens in 7,815 files, 17 block comments, about
  630 comment lines in 40 `.md` files, plus website and book pages. A
  token-level `jet fix` (lexer-driven, so strings and `://` stay intact) can do
  the rewrite mechanically. It touches most files in the repo and resets
  `git blame` for every comment line.
- **Lexer and registry:** `Scan.jet`, `Payload.jet` (E0002 text), registry rows
  `COMMENT_PREFIX`, `BLOCK_COMMENT_*`, a new doc-prefix row, teaching errors for
  `//` and `/*`, the `##`-needs-a-space error, and removal of the config `://`
  exception.
- **Hand-written scanners** that spell the comment glyph (`hardcode-census.mjs`
  found about 325 lines in about 110 files; an upper bound, including URL and
  foreign-language false positives). The real Jet-comment readers are in:
  - Lexer and parser: `Scan.jet`, `Payload.jet`, `Markers.jet:371`.
  - Driver and CLI: `Loader.jet:2829-2991`, `Entry.jet:509-557`,
    `Codemod.jet:553-562`, `Docs.jet:24/214/243/581`, `FixCommands.jet:73`,
    `SemIndexSymbols.jet:683`, `Bindings.jet:30`.
  - SemIndex: `Doctests.jet:27/131/157`, `SourceFacts.jet:378`.
  - Tools: devserver `EditActions*.jet`, `GraphJson.jet:347/367`; LSP
    `Links.jet:35`, `Sources.jet:244`; REPL `Input.jet:60-77`;
    `JetDebug/LineMap.jet:43`.
  - Foundation and Jetpack: `MarkerSource.jet:224`, `LintPolicy.jet:126`,
    `InlinePackage.jet`, `SourceScan.jet`, `PackageSourceEdit.jet`,
    `ManifestText.jet:154`, `RuntimeSourceExport.jet`.
  - Agent tools: `RuntimeGenerators.jet`, `CoreSourceRows.jet`.
- **Emitters:** the generators of `// runtime-init:`, `// jet-ffi-descriptor=`
  and `// generated by:`, plus doctest `// =>` (renamed `## =>`).
- **Self-host fixtures:** `tests/cases/selfhost_*/…/main.jet` embed copies of
  the lexer and must be regenerated.
- **Editors:** tree-sitter grammar and regenerated parser, both TextMate files,
  VS Code configuration, Zed configuration and queries.
- **Docs:** S5, S49, the language book, error explanations (E0002, E0042) and
  the formatter guide.

Estimate [INFERENCE]: the mechanical rewrite takes one to two days. Updating
the 30-plus scanners, emitters, editors and goldens, and a full self-host and
test cycle, takes about a week. The rewrite is easy; the long tail of
hand-written scanners is where bugs would hide.

## 7. Alternatives considered

The same program in each spelling (today's form is probe c14, which runs and
passes `jet fmt --check`):

**A — keep `//`, warn on Python's `//` habit (recommended)**

```jet
/// Splits a bill evenly.
#Inline fn share(total: Int, people: Int) -> Int {
    // whole-number division drops the cents
    total /% people  // #4828: round up later
}
half :: 7 // 2  // new warning: whole-number division is `/%`
```

**B — `##` comments, `###` docs, no block comments**

```jet
### Splits a bill evenly.
#Inline fn share(total: Int, people: Int) -> Int {
    ## whole-number division drops the cents
    total /% people  ## #4828: round up later
}
## `##Inline` (no space) is an error, never a silent comment
```

Exact rules for B: `##` followed by a space, a tab or a line end starts a
comment. `###` followed by the same starts a doc comment. Four or more `#`
form a plain comment. `##` followed by anything else is an error ("put a space
after `##`; a marker uses one `#`"). `//` and `/*` become teaching errors, and
`a // b` points to `/%`. The shebang rule and `#` inside comment text are
unchanged.

**C — single `#` comments, markers renamed (not balloted)**

```jet
#: Splits a bill evenly.        # (doc form would need a new glyph)
@Inline fn share(total: Int, people: Int) -> Int {
    # whole-number division drops the cents
    total /% people  # #4828 would be ambiguous: card ref or comment?
}
```

This is Python, shell and Nim familiarity. But it reverses D-META-HASH1,
D-PRE-COMPILE1 and the marker registry (`RULE_PREFIX`), and it renames
14,325 `#` tokens on top of the 95k comment lines. `[T#N]`, `pkg#1.2.0` and
`#!Name` need new spellings too. `@` is already taken: prefix `@` is the live
link (D-MEMREF1) and infix `@` the package-source reference (D-AT-INFIX1,
`Spellings.jet:26`). The tower-ballot rules forbid an option that
contradicts ratified law without the owner reopening it, so C is studied here
and left off the ballot.

**Also considered and rejected:**

- `;` comments (Lisp, assembly): `;` is the home-row key on US and is never
  typed in Jet (E0373). But C-family users' `a := 1; b := 2` would silently
  drop `b := 2`, which is far worse than today's error.
- `--` (Lua, SQL, Haskell): collides with `a - -b` and reads as decrement.
- Keeping `//` but making docs `##`: two families for one feature.
- Giving the freed `//` to floor division and retiring `/%`: only meaningful
  if B wins, and it would reopen a ratified operator. Leave it to a follow-up
  ballot if the owner picks B.

**Where the owner's "meta" instinct does fit:** the 169 machine-read comments
in §2 (`// runtime-init:`, `// jet-ffi-descriptor=`, `// jet:line`, doctest
`// =>`) are compiler-facing metadata hiding in comments. Turning them into
real `#` forms would make "`#` = the compiler acts here" more true. That is a
separate card, not this ballot.

## 8. Recommendation

**A: keep `//`, `///` and nested `/* */`, and add the Python floor-division
warning.**

- Nothing ratified or proposed needs `//`. Its natural rival meaning is
  already `/%`.
- `#` keeps one meaning, "the compiler acts here". That matches every
  language that uses `#` for compiler work, including Jai.
- There is no silent-marker-drop hazard, no 95k-line rewrite and no blame
  reset, and every editor and fenced-code highlighter already understands `//`.
- The real `//` problem (c11) gets a targeted, type-checked warning. None of
  the 61 label comments found by the lexical census would trigger it.

If the owner still prefers `##`, option B is fully specified and its
safety hole is closed: the space-after-`##` error keeps `##Pre` loud.

## 9. Probes

All in `probes/`. Outputs are in `probes/out.txt`, each with a header naming
compiler `1f70b8d4bd91cb88f4eda995388d67e288aac8df`. Runner: `node run.mjs
run|check|raw …`. Census scripts (read-only, at the top of the asset
directory): `census.mjs`, `docs-census.mjs`, `hardcode-census.mjs`,
`directive-census.mjs` and `floor-warn-census.mjs`. Their outputs are in
`probes/*.json` and `probes/hardcode-census.txt`. `probes/docs/index.md` is
the `jet doc` output for c06.

`out.txt` keeps two failed first runs. The first c05 run used `#line`, which
was later moved to c05b, and the first c06 run used the retired `pub`. Both
files were then fixed and rerun, and the later runs in `out.txt` are of the
files as stored.

| Probe | Shows | Result |
|---|---|---|
| c01_today_forms | all forms; `//` and `#` inside text; URL; `////`; commented marker | runs: `4`, `3.5`, text, URL |
| c02_hashhash_today | `##` line today | E0003 "Expected a name after `#`", so `##` is free |
| c03_hash_today | `#` line today | E0003 |
| c04_shebang | `#!` mid-file | E0042 |
| c05_hash_sites | `#Inline`, `[Int#3]` | runs: `6`, `3` |
| c05b_hash_line | ratified `#line` | E0927: not built yet |
| c06_doc_heading | `/// ## Examples` and a doc fence | runs. `jet doc` renders the summary and the doctest (`probes/docs/index.md`) |
| c07_unclosed_block | nested, unclosed `/*` | E0002 |
| c08_slash_uses | `/`, `/%`, `/%=`, positional `/` | runs: `8`, `4.25`, `4`, `12` |
| c09_fmt_last_comment | comment as last line of a body | **fmt moves it outside the function** |
| c10_fmt_block_comment_gap | blank line around a top-level block comment | fmt deletes the blank line after it |
| c11_python_floor_div | `half :: 7 // 2` | **prints 7, exit 0** |
| c12_hash_typo_marker | `##Pre(…)` today | E0003 (would be a silent comment under `##`) |
| c13_ballot_program | ballot program with `#{name}`, `#line`, `#Inline` in comments | runs: `333` |
| c14_ballot_program_fmt | formatted ballot program | runs; `fmt --check` passes |

## 10. Defects found (reported, not fixed)

1. **Formatter moves a body's final comment out of the function.** Probe c09:
   `fn run() { print(1)\n    // last line\n}` formats to `}` followed by
   `// last line` at top level. c01 shows the same for two trailing lines.
   This changes what the comment is attached to.
2. **Formatter deletes the blank line after a leading comment block** (c01
   after `//!`, c10 after `/* */`, c13 after the header). A file header then
   merges into the first item's comment or doc block. It may be intended; it
   needs a ruling.
3. **Registry bypass:** `COMMENT_PREFIX` and `BLOCK_COMMENT_OPEN/CLOSE`
   (`CoreSurface.jet:189-195`) have no Jet consumer. The lexer and about 30
   scanners hard-code `"//"` and `"///"`, and there is no registry row for `///`.
4. **tree-sitter block comments do not nest:**
   `Tools/editors/tree-sitter/grammar.js:523` uses the C regex, but S5 says
   block comments nest.
5. **TextMate grammars have no block-comment rule:**
   `Tools/editors/vscode/syntaxes/jet.tmLanguage.json:77-84` (and the copy in
   `Tools/editors/jet.tmGrammar`) highlight `/* */` as code.
   `vscode/language-configuration.json` has no `blockComment`.
6. **Unratified `//!` form in use:** 53 lines (e.g.
   `Examples/features/basics/digits.jet:1-3`). S49 names only `///`.
7. **Silent Python floor division (design gap, not a bug):** c11. `7 // 2`
   compiles to 7 with no diagnostic.
8. **Spec gap:** `/%` (ratified 2026-08-05 per the registry comment) has no row
   in `Docs/spec/syntax-decisions.md`.
