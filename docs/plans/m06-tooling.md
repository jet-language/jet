# M6 — Tooling I: fmt, test, new, multi-file, LSP v0

**Blocked on decisions:** S43 (test syntax), S44 (fmt style constants),
S49 (doc comments). S15 and S16 are **ratified** (see docs/02 Staged
implementation); M6 implements them. Depends on M3–M5 (fmt must handle
the whole surface).
**Error codes:** E0601+ (imports/visibility), tool exit codes below.

This milestone is four separable phases; implement and commit in order.
Each phase has its own exit criterion and can be a separate agent run if
needed (prompts: "Implement M6 phase N per docs/plans/m06-tooling.md").

## Phase 1 — `jet fmt`

One true style, zero configuration (philosophy #4). Style constants
(S44, recommended): 4-space indent; same-line `{`; one statement per
line; single blank line max between items; spaces around binary
operators; no space before `;`/`,`/`(` of a call; line width 100 with
simple argument-per-line overflow; trailing `;` per S6 untouched.

- `jet fmt file.jet` rewrites in place; `--check` exits 1 on diff
  (CI mode), printing a unified diff.
- Implementation: pretty-print from the AST **with comments preserved**.
  The lexer must start retaining comment tokens with spans; attach each
  comment to the nearest following node (or trailing on same line).
  This is the hardest part of the phase — write idempotence tests first.
- fmt also canonicalizes S14 foreign spellings when the parser's
  teaching-error recovery produced a valid AST (e.g. `let` → `val`):
  the recovery path already knows the canonical form; fmt just prints it.
- **Exit:** `jet fmt` is idempotent (fmt(fmt(x)) == fmt(x)) across every
  file in examples/ and tests/ui/*.fixed.jet, enforced by a new test.

## Phase 2 — `jet test` + `jet new`

Test syntax (S43, recommended — first-class blocks, only at top level):

```jet
test "parse_age accepts plain digits" {
    assert(parse_age("42") == ok(42));
    assert_eq(parse_age(""), err(ParseError.Empty));
}
```

- `test "name" { … }` parses like a parameterless fn body; only allowed
  at top level (E0601 elsewhere). Duplicate names → E0105.
- `assert_eq(a, b)` joins `assert` (M4): on failure prints both values
  ("left: …, right: …") then the runtime report.
- `jet test file.jet` (or a directory) compiles ONE binary containing
  all tests + a tiny generated harness (no cargo, R9): each test runs,
  failures are caught per-test via `catch_unwind` **inside generated
  code only** (the runtime helper may use it; user code still never
  unwinds observably). Output: one line per test, `pass`/`FAIL`, summary
  line, exit 1 on any failure. Normal `jet run`/`build` ignores test
  blocks entirely.
- `jet new name` creates `name/` with `main.jet` (hello world) and
  `.gitignore` (`build/`). Nothing else — no manifest (R9; manifests are
  M12 and opt-in).
- **Exit:** a failing-then-fixed test example; goldens pin `jet test`
  output shape.

## Phase 3 — Multi-file programs (S16 + S18 enforcement)

```jet
import "grades/scoring";         // file: scoring.jet beside this file's tree
import scoring;                   // module: find scoring.jet under project root
import scoring as gradebook;      // same module, different namespace

fn main() {
    print(scoring.letter(91));
    print(gradebook.letter(92));
}
```

- **File import** `import "<path>" [as alias];`: path relative to the
  **importing file's directory**; `.jet` appended; subdirs ok
  (`"util/text"`). Default namespace = last path segment. No `..` past
  the entry file's directory tree (E0602). Missing file → E0603.
- **Module import** `import <name> [as alias];`: search recursively from
  **project root** (entry file's directory, or `jet.toml` dir when M12)
  for `name.jet` or `name/{name,main}.jet`; skip `build/`, `target/`,
  dot-dirs. Ambiguous duplicates → E0606 (lists paths). Default
  namespace = `name`.
- Import cycles → E0604 (prints the cycle). Reach items as
  `namespace.item`; only `pub` items visible (E0605). `pub` on fields
  gates cross-file field access/construction (finishes M3 rule 2).
- Compilation model: the driver parses the import graph, sema checks the
  whole program (modules are namespaces, not separate crates), codegen
  emits ONE Rust file with `mod` blocks. Name mangling becomes
  `user_<module>_<name>` internally; `main` excepted.
- `jet run entry.jet` keeps working unchanged for single files (R9).
- **Exit:** a 3-file example program; ui fixtures for E0602–E0605.

## Phase 4 — `--small` profile + LSP v0

- `jet build --small`: `opt-level="z"`, full LTO, and the S15-ratified
  panic stance. Exit criterion: measurably smaller binary than default
  on examples/16_wordcount.jet (a test asserts the size relation, not
  absolute numbers).
- **LSP v0** (`jet lsp`, stdio JSON-RPC): scope is exactly
  (a) publish full-document diagnostics on open/change — reusing the
  real compiler front end in-process, (b) code actions that apply S14
  autocorrects (the quick-fix payload comes from the teaching error's
  known canonical form), (c) formatting via Phase 1.
  Implementation: hand-rolled JSON (de)serializer for the ~6 message
  types used (I6: no serde without owner approval — if this proves
  miserable, STOP and ask the owner to approve serde_json for the
  tooling binary only, never the compiler core).
  Defer everything else (completion/hover/goto) to M13.
- A minimal VS Code extension lives in `editors/vscode/` (TextMate
  grammar for highlighting + LSP client pointing at `jet lsp`). Plain
  JSON/JS, no build step.
- **Exit:** scripted LSP test: send didOpen with `let x = 1;`, receive
  the E0009 diagnostic + a quick-fix edit that turns it into `val x = 1;`;
  autocorrect turns a pasted C-style snippet into canonical Jet.

## Diagnostics to register

E0601 `test` block in wrong position · E0602 import path escapes the
project · E0603 imported file not found · E0604 import cycle ·
E0605 item exists but is private · E0606 ambiguous module name (lists paths).
Teaching: E0015 (`use` → `import`) message updates to point at the real
feature; E0030 `mod`/`require`/`include` → `import`.

## Out of scope

Package manager & registries (M12), `import` of anything but local
files, fmt configuration of any kind, test filtering/parallelism beyond
"run them all", watch modes, LSP features beyond the three listed.
