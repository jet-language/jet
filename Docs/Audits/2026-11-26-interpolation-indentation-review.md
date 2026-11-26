# Design Review: Interpolation That Keeps Indentation

- **Date:** 2026-11-26
- **Scope:** Multi-line interpolation indentation in triple-quoted string literals (`"""…"""`)
- **Status:** Ratified design, implemented and verified across native tiers (`-O0`, `-O1`, `-O2`), comptime, `print`, and WebAssembly

## 1. Problem Statement

Jet's triple-quoted string literals (`"""…"""`) strip the closing delimiter's
indentation from every line so multi-line templates can sit naturally inside
indented functions and blocks. Before this change, however, interpolating a
multi-line `String` value into an indented hole broke the visual alignment of
every line after the first:

```jet
body: "x: 1\ny: 2"
code: """
fn main() {{
    {body}
}}
"""
```

Line 1 of `body` (`x: 1`) sat at four spaces because of the literal prefix before
`{body}`, while line 2 (`y: 2`) dropped back to column 0. Every code generator,
SQL/config template, and nested `"""…"""` helper had to manually call
`.replace("\n", "\n    ")` at every hole and duplicate the indentation width in
two places.

## 2. Adversarial Attacks on the Proposal

### Attack 1: Mid-line holes and multiple holes on the same line

A naive implementation that measures the column offset of `{` produces broken
output for mid-line holes such as:

```jet
"""
fn main() {{
    // {note}
}}
"""
```

Measuring the hole's column (`7` spaces, matching `"    // "`) would indent line
2 of `note` past the comment prefix by an arbitrary column count rather than to
the line's indentation level. Worse, two holes on a column-0 line
(`{a}    {b}`) would falsely treat the spaces between `{a}` and `{b}` as
indentation for `{b}`.

**Resolution:** Track `(indent, leading_ws)` per line across the lexer-dedented
literal parts of the triple-quoted string:
- Reset `indent = ""` and `leading_ws = true` at every `\n`.
- While `leading_ws` is `true`, accumulate `' '` and `'\t'` into `indent`.
- On any non-horizontal-whitespace byte OR upon encountering any interpolation
  hole, set `leading_ws = false` while preserving `indent` for the remainder of
  the line.

Result:
- `    // {note}` gives `{note}` leading indent `"    "`.
- `    {a}: {b}` gives both `{a}` and `{b}` leading indent `"    "`.
- `{a}    {b}` on a column-0 line gives both `{a}` and `{b}` leading indent `""`
  (no wrapping).
- Debug-label holes (`    {body=}`) emit the label text after the leading
  whitespace, so `leading_ws` turns `false` while `indent` stays `"    "`.

### Attack 2: Blank lines, consecutive blank lines, trailing newlines, and CRLF

A naive rewrite of `"\n"` to `"\n{indent}"` injects trailing spaces onto blank
lines (`"a\n\nb"` becomes `"a\n    \n    b"`) and onto the empty line after a
trailing newline (`"a\nb\n"` becomes `"a\n    b\n    "`). A follow-up patch
like `.replace("\n{indent}\n", "\n\n")` fails on consecutive blank lines
(`"a\n\n\nb"`, because matches overlap) and fails on CRLF (`"\r\n"`).

**Resolution:** Walk lines once via `raw.split("\n")`:
- Line 0 is written as-is (the template's own prefix already precedes the hole).
- For every subsequent line `line`: if `line == ""` or `line == "\r"`, append
  `"\n{line}"` with no indentation prefix; otherwise append
  `"\n{indent}{line}"`.

This handles arbitrary runs of consecutive blank lines (`\n\n\n`), trailing
newlines (`"one\ntwo\n"`), leading newlines (`"\none\ntwo"`), and CRLF
(`"r1\r\nr2\r\n\r\nr3"`) with zero trailing whitespace and exact line-ending
preservation.

### Attack 3: Single-quoted strings, raw backtick strings, and checked text heads

Indentation keeping must apply **only** to plain triple-quoted `String` literals:
- Single-quoted strings (`"header\n    {body}"`) do not perform closing-delimiter
  dedent and must preserve exact literal bytes with no hidden line rewriting.
- Raw backtick strings do not interpolate at all.
- Checked text heads (`SQL{"""…"""}`, `HTML{"""…"""}`, `Sh{"""…"""}`) parse as
  `Expr.TypedLit`, not `Expr.Str`. Their holes are bound parameters, escaped
  HTML/attribute slots, or shell argv elements; rewriting hole values would
  silently corrupt SQL parameters, shell arguments, or HTML escaping.

Identifying triple-quoted literals by checking the opening three bytes of the
literal's source span (`"""`) in `JetSema` (`sema_expr_is_triple_quoted`) is
exact and requires zero AST or wire-format changes.

### Attack 4: Cross-tier parity (I9), pure/memo functions, and latent `JetIR` bug

Lowering indentation keeping in `JetSema` (`Compiler/JetSema/Source/Sema/Expressions/StringIndent.jet`)
into a pure `TExprKind.InlineBlock` built solely from existing checked-tree
primitives (`TBuiltinOp.Contains`, `TBuiltinOp.Split`, `TStmt.ForIn`, `StrLit`)
gives automatic parity across:
- Native `-O0`, `-O1`, and `-O2` (and `JetIR` lint)
- Comptime module constants and `#run` expressions
- Direct `print("""…""")` (`ir_print_line` consumes the same `TStrPart` values)
- WebAssembly (`--target web`)
- Pure `-[]>` and `#Memo` functions
- Zero new runtime exports

**Bug uncovered during live native verification:** `JetIR`'s `ir_text_walk`
(`Compiler/JetIR/Source/BuildLazy.jet`) uses `sep_node == -1` as the sentinel
for `text.lines()`, while `text.split(sep)` passes the `BodyNode` ID of `sep`.
Because compiler-generated expressions flatten to negative `BodyNode` IDs
(`TEXPR_FIRST_FLAT_ID - k < 0`), `splits: sep_node >= 0` misclassified
compiler-generated `text.split("\n")` loops as `text.lines()`
(`jet_rt_text_line_step`), silently dropping trailing newlines and stripping
`\r`. Fixing the predicate to `splits: sep_node != -1` resolved the root bug
for all compiler-generated `split` walks.

### Attack 5: Runtime overhead on single-line and scalar holes

Most indented interpolation holes in real programs hold numbers, booleans, or
single-line strings:
- **Compile-time scalar skip:** When an interpolation has no format selector
  that could produce multiple lines and its type is a newline-free scalar
  (`Int`, `Float`, `Float32`, `Bool`, `IntN`, `InlineRange`, `Quantity`,
  `Measure`), `JetSema` emits no wrapper at all (zero instructions).
- **Runtime single-line fast path:** For `String`, `Char`, user `Display`
  values, and format selectors (`:Pretty`, `:Debug`, etc.), the lowered block
  checks `!__raw.contains("\n")` first and returns `__raw` untouched with no
  splitting or reallocation.
- **Linear multi-line path:** Multi-line values stream through `JetIR`'s fused
  zero-list-allocation `ir_text_walk` and in-place O(n) `ir_self_append`.

## 3. Repository Impact Sweep

A sweep across the repository found 335 existing indented interpolation holes
inside triple-quoted strings (compiler generators, tests, tools, and examples).
Inspecting them showed single-line scalar and identifier values (`{i}`,
`{nonce}`, `{poll_ms}`, `{q(id)}`), which either skip wrapping at compile time
or take the single-line fast path with identical output. Zero existing tests or
examples change behavior.

## 4. Verification Summary

- **Sema fixtures:** `Compiler/JetSema/Tests/InterpolationIndentFixtures.jet`
  (4/4 passing) verifies triple-quoted indented holes, mid-line holes, multiple
  holes per line, column-0 lines, tab indentation, single-quoted exclusion,
  scalar zero-cost skip, `Char`, custom `Display`, `:Debug`, pure `-[]>`, and
  `#Memo`.
- **End-to-end tier parity:** `Tests/Cases/TextStorage/multiline_interpolation_keeps_indentation.case`
  verifies all edge cases (basic, mid-line, multi-hole, 3-level nested
  templates, consecutive blank lines, trailing/leading newlines, CRLF, tabs,
  fast path/scalars, custom `Display`, `:Pretty`, single-quoted exclusion, pure
  and `#Memo`, comptime constants and `#run`, and `print("""…""")`) across
  `-O0`, `-O1`, `--release` (`-O2`), and `--target web`.
- **Golden example:** `Examples/Basics/MultilineInterpolationIndent.jet` and
  `Examples/Basics/MultilineInterpolationIndent.out`, verified byte-for-byte
  across `-O0`, `-O1`, and `-O2`.
