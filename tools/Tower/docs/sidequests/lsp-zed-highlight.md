# Plan: Fix Zed syntax highlighting (c41)

**Status:** Ready to implement — no open upstream gates.

---

## Goal

Zed shows partial or no syntax highlighting for `.jet` files; VSCodium works.
Rewrite the tree-sitter grammar to match all ratified syntax (as of 2026-06-22),
rebuild the WASM, and update `highlights.scm`.

---

## Why VSCodium works and Zed does not

VSCodium uses `editors/jet.tmGrammar` — a set of regex patterns that match
tokens in isolation. It never needs to parse a full program tree.

Zed uses tree-sitter exclusively. Every highlight query runs against the parse
tree. When tree-sitter cannot parse a construct, it emits an `ERROR` node and
the query captures nothing inside it — those tokens render unstyled.

**Root cause in one sentence:** the tree-sitter grammar (`editors/tree-sitter/grammar.js`)
was frozen on 2026-06-17 and the `grammars/jet.wasm` was compiled that day, the
day before a wave of syntax ratifications (S6-R, D-BIND1, S19-amend, D-IF1)
changed the surface; the TextMate grammar was kept current while the tree-sitter
grammar was not.

---

## Is this trivially fixable?

**No — a grammar rewrite and WASM rebuild are required. There is no patch
shorter than fixing the grammar.**

Verified with `tree-sitter parse` on the owner's machine:

```
$ tree-sitter parse examples/features/01_hello.jet
(source_file
  (comment)
  (function_def name: (identifier)
    (param_list)
    (block
      (expr_stmt (call_expr name: (identifier) (string_literal)))))
          MISSING ";" [2, 25] - [2, 25])
```

The grammar **does load** (no WASM ABI failure; `grammars/jet.wasm` is valid).
The problem is purely content staleness. Parsing a file with `::` / `:=`
bindings confirms:

```
(ERROR [0, 10] - [3, 18]      ← everything inside becomes ERROR
  (ERROR [1, 4] - [3, 13] …)
```

Every statement the compiler accepts — `x :: 42`, `y := "hello"`,
`loop i in 1..5 { }` — lands inside an `ERROR` node. The highlight queries get
no matches inside `ERROR` nodes, so Zed shows nothing.

**Fast-track options:**

There are none. The grammar must be rewritten and the WASM rebuilt. The rebuild
itself is mechanical (`FORCE=1 editors/zed/install.sh`); the grammar rewrite is
the actual work. Time estimate: a few hours to rewrite `grammar.js` and
`highlights.scm`, plus the `tree-sitter generate` + WASM compile (~1 min).

---

## Current state (files involved)

| File | Problem |
|---|---|
| `editors/tree-sitter/grammar.js` | Stale grammar — root source, fixed here |
| `editors/zed/grammar-repo/grammar.js` | Copy synced by `install.sh` — rebuilt |
| `editors/zed/grammar-repo/src/` | Compiled parser (grammar.json, parser.c) — regenerated |
| `editors/zed/grammars/jet.wasm` | Prebuilt WASM from 2026-06-17 — rebuilt |
| `editors/zed/languages/jet/highlights.scm` | Stale capture queries — fixed here |
| `editors/zed/extension.toml` | Machine-specific `file://` URI — portability bug |

---

## Root causes

**R1 — Explicit `";"` in every statement rule; S6-R users never type them.**

S6-R (ratified 2026-06-18) removed visible semicolons. The lexer inserts them
synthetically; tree-sitter parses raw file text which has none. Every
`val_stmt`, `return_stmt`, `break_stmt`, `continue_stmt`, `expr_stmt`,
`struct_field`, `enum_variant`, `const_def`, and `switch_arm` emits
`MISSING ";"`. Even the simple `fn main() { print("hello") }` file produces a
MISSING node, causing Zed to apply error-recovery that discards the highlight
captures inside `block`.

Fix: make `;` optional (`optional(";")`).

**R2 — Grammar uses retired `val`/`var`/`while`/`for`/`switch`; misses `::` / `:=` / `loop`.**

D-BIND1 (2026-06-18) retired `val`/`var`; bindings are now `name :: expr` and
`name := expr`. S19-amend (2026-06-17) retired `while`/`for`; loops unify under
`loop`. D-IF1/S24 (2026-06-18) retired `switch`; multi-arm dispatch is
`if subject { arm -> body }`.

The grammar has `val_stmt`, `while_stmt`, `for_stmt`, `switch_stmt` with the old
keywords. All new-syntax files produce `ERROR` nodes. No `bind_stmt`, no unified
`loop_stmt`, no `if`-arm form exist.

**R3 — `impl_block` uses `:` for the trait connector; S83 ratified `~~`.**

Grammar line 115: `optional(seq(":", field("trait", $.type_identifier)))`.
S83 (ratified 2026-06-19) chose `~~`: `impl Point~~Drawable { }`. The `:` form
was never ratified; it also collides with type-annotation syntax.

**R4 — `highlights.scm` lists retired keywords as `@keyword`; misses new sigils and markers.**

Lines 24–28 list `"val"`, `"var"`, `"while"`, `"for"`, `"switch"` in the
keyword capture. Missing: `"::"`, `":="`, `"~~"`, `"loop"` (there but `while`/`for`
should be absent), and any rule for `#`-prefixed markers (`#Unsafe`, `#Pure`,
`#Test`, `#Todo`, `#Context`, `#[Serialize, …]`).

The `val_stmt` name-capture on line 36 also breaks because the node is gone.

**R5 — `grammar-repo` submodule pointer is dangling.**

The outer repo HEAD records grammar-repo at `9c856419ddcd636d8b80373ad7b67dfbdbbc1605`.
That commit does not exist in `editors/zed/grammar-repo/.git` (only `4886d41` and
`7055f83` are present). `install.sh` generates `extension.toml` from whatever
`git rev-parse HEAD` returns in the working tree (`4886d41`), so it works on this
machine. A fresh `git submodule update` would leave the repo broken.

**R6 — `extension.toml` `repository` is a hardcoded absolute `file://` path.**

Generated `extension.toml` contains:
```
repository = "file:///home/nate/Projects/Github/jet/editors/zed/grammar-repo"
```
This is machine-specific. Any second developer or CI run produces a different
path; the committed file is unusable by anyone else.

**R7 — No `#`-prefixed marker support in grammar or highlights.**

`#Unsafe`, `#Pure`, `#Test`, `#Todo`, `#Context(…)`, `#Audit("…")`,
`#[Serialize, …]` (all ratified: D-ATTR1, D-CASING1, S58, S43, S60, D-TOOL2)
have no grammar node and no highlight rule. They render as plain text.

---

## Approach

### Step 1 — Rewrite `editors/tree-sitter/grammar.js`

This is the authoritative source. `grammar-repo/` is a copy synced by
`install.sh`.

**1a. Make `;` optional everywhere (R1).**

Change every trailing `";"` in statement and field rules to `optional(";")`:
- `use_stmt`, `struct_field`, `enum_variant`, `const_def`
- `assign_stmt`, `return_stmt`, `expr_stmt`
- `break_stmt`, `continue_stmt`
- `comptime_stmt` (remove entirely — comptime blocks under D-WHEN1 use different form)
- `switch_arm`, `switch_else` (kept until If-arm form added — see 1d)

**1b. Replace `val_stmt` with `bind_stmt` for `::` / `:=` (R2).**

Remove `val_stmt`. Add:

```js
bind_stmt: ($) =>
  choice(
    // immutable: name :: expr  or  name: Type :: expr
    seq(
      field("name", $.identifier),
      optional(seq(":", field("type", $._type))),
      "::",
      field("value", $._expr),
      optional(";")
    ),
    // mutable: name := expr  or  name: Type := expr
    seq(
      field("name", $.identifier),
      optional(seq(":", field("type", $._type))),
      ":=",
      field("value", $._expr),
      optional(";")
    )
  ),
```

Update `_stmt` to list `$.bind_stmt` in place of `$.val_stmt`.

Keep `"val"` and `"var"` recognizable by adding them to `_stmt` as an error node
so they don't cause a total parse failure:

```js
foreign_bind_kw: (_) => seq(choice("val", "var"), $.identifier),
```

Add `$.foreign_bind_kw` to `_stmt` as a low-priority alternative.

**1c. Replace `while_stmt` / `for_stmt` with unified `loop_stmt` (R2).**

Remove `while_stmt` and `for_stmt`. Rename the existing `loop_stmt` (currently
just `seq("loop", $.block)`) to the full unified form:

```js
loop_stmt: ($) =>
  seq(
    "loop",
    optional(
      choice(
        field("cond", $._expr),
        seq(
          field("var", $.identifier),
          optional(seq(",", field("var2", $.identifier))),
          "in",
          field("iter", $._expr)
        )
      )
    ),
    $.block
  ),
```

**1d. Add `if`-arm dispatch; remove `switch_stmt` (R2).**

Extend `if_stmt` to handle the multi-arm form:

```js
if_stmt: ($) =>
  seq(
    "if",
    field("subject", $._expr),
    choice(
      // Boolean if: cond { } else { }
      seq(
        field("then", $.block),
        optional(seq("else", choice($.if_stmt, $.block)))
      ),
      // Multi-arm dispatch: if subject { arm -> body … }
      seq(
        "{",
        repeat($.if_arm),
        optional($.if_else_arm),
        "}"
      )
    )
  ),

if_arm: ($) =>
  seq(
    field("cond", $._expr),
    "->",
    choice($.block, $._expr),
    optional(";")
  ),

if_else_arm: ($) =>
  seq("else", "->", choice($.block, $._expr), optional(";")),
```

Remove `switch_stmt`, `switch_arm`, `switch_else`.

**1e. Fix `impl_block` to use `~~` connector for S83 (R3).**

```js
impl_block: ($) =>
  seq(
    "impl",
    field("type", $.type_identifier),
    optional(seq("~~", field("trait", $.type_identifier))),
    "{",
    repeat($.function_def),
    "}"
  ),
```

Same change in `trait_impl_block` (inline in struct/enum body).

**1f. Add `marker` node for `#Keyword` and `#[list]` (R7).**

```js
marker: ($) =>
  seq(
    "#",
    choice(
      seq($.type_identifier, optional(seq("(", commaSep($._expr), ")"))),
      seq("[", commaSep($.type_identifier), "]")
    )
  ),
```

Add `$.marker` to the `extras` array so markers can appear before items and
statements without requiring explicit grammar positions.

**1g. Add `block_comment` for `/* … */` (S5 amended 2026-06-15).**

```js
block_comment: (_) =>
  token(seq("/*", /[^*]*\*+([^/*][^*]*\*+)*/, "/")),
```

Add `$.block_comment` to `extras` alongside `$.comment`.

**1h. Update `param` capability keywords for D-CAP1.**

D-CAP1 reserves `edit`/`share`. Add to the capability prefix:

```js
param: ($) =>
  seq(
    optional(choice("mut", "take", "view", "ref", "edit", "share")),
    field("name", $.identifier),
    ":",
    field("type", $._type)
  ),
```

### Step 2 — Update `editors/zed/languages/jet/highlights.scm`

Replace the stale keyword list with:

```scheme
; Binding sigils (S2 / D-BIND1)
"::"  @operator
":="  @operator

; Trait / external-definition connector (S83)
"~~"  @operator

; Arm arrow and lambda arrow
"->"  @operator
"=>"  @operator

; Keywords — live Jet keywords only (no FOREIGN_* forms)
[
  "fn" "return" "if" "else" "loop" "in"
  "break" "continue" "struct" "enum" "impl"
  "trait" "pub" "use" "const" "comptime"
  "extern" "rust" "module" "distinct" "region"
  "step"
] @keyword

; Markers / attributes (#Unsafe, #Pure, #[Serialize, …])
(marker) @attribute

; Function definitions
(function_def name: (identifier) @function)
(call_expr name: (identifier) @function.call)
(method_call_expr method: (identifier) @function.method)

; Variable bindings (updated from val_stmt → bind_stmt)
(bind_stmt name: (identifier) @variable)
(param name: (identifier) @variable.parameter)
(lambda_param name: (identifier) @variable.parameter)
(loop_stmt var: (identifier) @variable)

; Identifiers
(identifier) @variable
```

Remove the stale lines that referenced `"val"`, `"var"`, `"while"`, `"for"`,
`"switch"`, `"comptime"` as keywords, and the `(val_stmt …)` and `(for_stmt …)`
captures.

Note: Do not add `@warning` for retired keywords. `@warning` is not a standard
Zed/tree-sitter highlight capture name; it maps to nothing in most themes and
renders the same as plain text. The LSP already flags retired forms with
diagnostic squiggles; the grammar's job is to parse valid code cleanly.

### Step 3 — Regenerate the parser and WASM

```bash
FORCE=1 nix develop -c editors/zed/install.sh
```

This:
1. Copies `grammar.js` from `editors/tree-sitter/` into `grammar-repo/`.
2. Runs `tree-sitter generate` (updates `grammar.json`, `node-types.json`,
   `parser.c`).
3. Builds `grammars/jet.wasm`.
4. Commits the grammar-repo and records the new SHA.
5. Regenerates `extension.toml` from `extension.toml.in`.

### Step 4 — Fix the submodule pointer (R5)

After `install.sh` commits inside `grammar-repo/`:

```bash
git add editors/zed/grammar-repo editors/zed/grammars/jet.wasm
git commit -m "editors/zed: rewrite tree-sitter grammar for S6-R/D-BIND1/S19-amend/D-IF1/S83; rebuild WASM"
```

This clears the dangling `9c85641` reference.

### Step 5 — Fix `extension.toml` portability (R6)

Add `editors/zed/extension.toml` to `.gitignore` for that directory. Every
developer must run `install.sh` before adding the dev extension; that is already
documented in `editors/zed/README.md`.

```bash
echo "extension.toml" >> editors/zed/.gitignore
git rm --cached editors/zed/extension.toml   # remove the tracked file
git add editors/zed/.gitignore
```

The long-run fix is publishing `grammar-repo/` to a real remote repo and
replacing the `file://` URI in `extension.toml.in` with the GitHub URL. That
enables submission to the Zed extension registry. It is a separate ops task, not
gated on a syntax decision. (See surfaced decisions.)

### Step 6 — Verify

1. `nix develop -c cargo build` → produces `target/debug/jet`.
2. `FORCE=1 nix develop -c editors/zed/install.sh` → clean rebuild.
3. Confirm no `MISSING` or `ERROR` nodes:
   ```bash
   cd editors/zed/grammar-repo
   nix develop -c tree-sitter parse ../../.../examples/features/01_hello.jet
   # Expect: no ERROR or MISSING lines in output
   ```
4. Test new-syntax file:
   ```bash
   cat > /tmp/test.jet << 'EOF'
   fn demo() {
       x :: 42
       y := "hello"
       loop i in 1..5 {
           print(i)
       }
   }
   EOF
   nix develop -c tree-sitter parse /tmp/test.jet
   # Expect: bind_stmt nodes for x and y, loop_stmt with var field
   ```
5. In Zed: remove old Jet dev extension → Add Dev Extension → `editors/zed/` →
   reload window → open `examples/features/01_hello.jet`.
   - `fn` / `return` / `loop` → keyword colour
   - `main` → function colour
   - `"hello, world"` → string colour
   - No grey/unstyled blocks
6. Open a file with `x :: 42` and `y := "hello"` → `::` and `:=` coloured as
   `@operator`, names as `@variable`.
7. Open a file with `#Unsafe { }` → `#Unsafe` coloured as `@attribute`.
8. `nix develop -c jet lsp doctor` → LSP starts (binary discovery in
   `wasm-src/lib.rs` is unchanged).

---

## Diagnostics

No new compiler diagnostic codes. All changes are in `editors/`; `Source/` is
untouched.

---

## Tests

Add a tree-sitter corpus test at `editors/tree-sitter/test/corpus/basics.txt`:

```
==================
Immutable binding
==================

fn f() {
    x :: 1
}

---

(source_file
  (function_def
    name: (identifier)
    (param_list)
    (block
      (bind_stmt name: (identifier) value: (integer_literal)))))

==================
Loop iteration
==================

fn f() {
    loop i in 1..5 {
        print(i)
    }
}

---

(source_file
  (function_def
    name: (identifier)
    (param_list)
    (block
      (loop_stmt
        var: (identifier)
        iter: (binary_expr)
        (block
          (expr_stmt (call_expr name: (identifier) (identifier))))))))
```

Run with `nix develop -c tree-sitter test -C editors/tree-sitter`. Add this
invocation to the project Makefile or CI if one exists.

---

## Gate

None. All ratifications that this plan implements are complete: S6-R, D-BIND1,
S19-amend, D-IF1, S83, D-ATTR1, D-CASING1, D-CAP1.

The `~~` connector in `impl_block` (S83) is "ratified, not yet implemented" on
the *compiler* side — the grammar encoding it here is ahead of the compiler
parser, but that is correct: the grammar should track ratified syntax, not
compiler completeness.
