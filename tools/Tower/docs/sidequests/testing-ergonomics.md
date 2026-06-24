# c51 — Testing ergonomics: property tests, doctests, coverage
**Decisions:** D-TEST1 (ratified 2026-06-22), D-TEST4 (ratified 2026-06-22), D-COV1 (tooling-only, no syntax)
**Gate:** none — both syntax decisions are UNBLOCKED.

---

## What is decided

**D-TEST1 (option B):** A `#Test fn` with parameters is a property test; inputs are generated
from parameter types with automatic invisible shrinking. A `#Test fn` with no parameters is a
unit test. Zero new syntax — matches the S82 worked example.

**D-TEST4 (option A):** Code examples inside `///` doc comments (S49) run as tests. Expected
output is a `// =>` trailing comment on the producing line. A mismatch fires **E2901**. Reuses
the `//` comment marker (S5); no new tokens.

**D-COV1:** `jet test --coverage` is a tooling flag only — no syntax changes.

---

## Current state

`Source/Codegen/mod.rs` already emits `jet_test_N()` harness wrappers for `#Test` blocks (S43,
E2-M11). `Source/Sema/Registration.rs` registers `Item::Test` items in `CompileMode::Test`.
`Source/Codegen/TIR.rs` (`tir_covers_test_body`) gates test bodies into the TIR subset. The
parser (`Parser/Modules.rs`) parses `#Test "name" { … }` via `test_def()`. What is missing:

1. Property-test parameter parsing and generator wiring.
2. Doctest extraction from `///` comments.
3. `jet test --coverage` plumbing.

---

## Plan

### Phase 1 — Property tests (D-TEST1)

**AST (`Source/AST.rs`)**

`TestDef` (line 721) gains an optional `params: Vec<(String, Type)>` field — the named
parameter list. A `TestDef` with `params.is_empty()` is a unit test; non-empty is a property
test. Add:

```rust
pub struct TestDef {
    pub name: String,
    pub name_span: Span,
    pub params: Vec<(String, Type)>,   // NEW: empty = unit test
    pub body: Vec<Stmt>,
    pub span: Span,
}
```

**Parser (`Source/Parser/Modules.rs`)**

`test_def()`: after parsing the name string, optionally parse `(name: Type, …)` before `{`.
If present, populate `TestDef::params`. No new tokens — reuses the existing `(`, `:`, `,`,
`)` tokens already in the lexer.

**Sema (`Source/Sema/Registration.rs`, `CheckerItems.rs`)**

In `register_test` / `check_test_def`: if `params` is non-empty, validate each param type is
a concrete type that the generator table knows (Bool, Int, Float, String, Char, List(_), and
user structs whose fields are all generatable). Emit **E2301** ("property test parameter `x:
T` has no generator — use a concrete primitive or a struct whose fields are all primitive")
for unsupported types.

**Codegen (`Source/Codegen/mod.rs`, `TIR.rs`)**

`emit_test_harness` gains a `is_property: bool` path. For property tests, generate:

```rust
fn jet_test_N() -> Result<(), String> {
    let mut __rng = JetRng::from_entropy_seed();
    for __trial in 0..256u32 {
        let x: T = __gen_T(&mut __rng);
        // shrink loop on failure
        let result = std::panic::catch_unwind(|| { /* body with param bindings */ });
        if result.is_err() {
            return Err(format!("falsified on trial {}: x = {:?}", __trial, x));
        }
    }
    Ok(())
}
```

Generator dispatch (`__gen_T`) is a match on the type name, emitted inline — no trait
dispatch, no external crate (I6). Shrinking is a fixed-step bisect on integers and string
length, array length; structs shrink field-by-field. The shrink loop is emitted verbatim.
Trial count is a constant (`256`); expose `#Test(trials: N)` as a follow-on only if evidence
demands it — simplicity ratchet (I8).

`tir_covers_test_body` is extended to allow `#Test fn` bodies (which may call any sema-clean
Jet code, not just TIR-subset code). Property-test bodies use a separate codegen path that
doesn't go through TIR coverage gating.

**Syntax (`Source/Syntax.rs`)**

Add:
```rust
pub const TEST_DEFAULT_TRIALS: u32 = 256; // D-TEST1
```

**Example / golden test (I5)**

`examples/features/test_property.jet`:
```jet
#Test fn add_commutative(a: Int, b: Int) {
    assert(a + b == b + a)
}
```

Expected output (in `examples/features/expected/test_property.txt`):
```
test add_commutative ... ok (256 trials)
```

Add snapshot at `tests/ui/test_property_ok.txt`.

**Diagnostic (I4)**

E2301 — property test parameter type has no generator. Add entry to
`docs/spec/diagnostics.md` and snapshot at `tests/ui/e2301_no_generator.txt`.

---

### Phase 2 — Doctests (D-TEST4)

**Extraction (`Source/lib.rs` or new `Source/Doctest.rs`)**

Add `fn extract_doctests(src: &str) -> Vec<DoctestCase>` where:

```rust
pub struct DoctestCase {
    pub fn_name: String,
    pub code: String,           // the fenced ```jet block
    pub expected_lines: Vec<(usize, String)>, // (line_in_block, expected after // =>)
    pub span: Span,             // points into the original source for diagnostics
}
```

Walk `///` doc-comment lines (lexer token `TripleSlash` or string scan). Detect fenced
` ```jet ` … ` ``` ` blocks. Within each block, collect lines that end in `// => <value>`.

**Sema integration**

In `CompileMode::Test`, after normal sema, run `extract_doctests` over every module's raw
source. For each `DoctestCase`, synthesize a `TestDef` with a unique name
`"doctest::<fn_name>::<line>"`, wrap the code block in a synthetic `#Test` body that calls
`assert_eq!(format!("{:?}", <expr>), "<expected>")`, and type-check it. A mismatch fires
**E2901**:

```
error[E2901]: doctest output mismatch
  --> src/math.jet:12:5
   |
12 |     add(2, 3) // => 6
   |     ^^^^^^^^^ expected `6`, got `5`
```

**Codegen**

Doctest cases are emitted as additional `jet_test_N()` fns alongside regular tests.

**Diagnostic (I4)**

E2901 — doctest output mismatch. Add to `docs/spec/diagnostics.md`; snapshot at
`tests/ui/e2901_doctest_mismatch.txt`.

**Example (I5)**

`examples/features/test_doctest.jet`:
```jet
/// Returns the sum of two integers.
///
/// ```jet
/// add(2, 3) // => 5
/// ```
fn add(a: Int, b: Int) -> Int { a + b }
```

Golden test verifies `jet test examples/features/test_doctest.jet` exits 0.

---

### Phase 3 — Coverage (`jet test --coverage`, D-COV1)

No syntax. Tooling only — `Source/CmdDevTools.rs`.

Add `--coverage` flag to `jet test` parsing (`Source/main.rs`). When set:

1. Compile normally to Rust; inject `#[cfg(coverage)]` / `#[allow(dead_code)]`-annotated
   instrumentation (use `cargo llvm-cov` under Nix if available, else `grcov`). The flag is
   a best-effort wrapper — if neither tool is on PATH, print a teaching message naming both
   and exit 0 (not a hard error).
2. After the test binary runs, shell out to format the LCOV report and print a one-line
   summary `coverage: N% (M/K lines)` to stderr.
3. Write `jet-coverage/` report directory.

This is pure tooling glue — no sema, no AST changes. No external Rust crates in `Source/`
(I6); the coverage tool is invoked via subprocess.

---

## Files touched

| File | Change |
|------|--------|
| `Source/AST.rs` | `TestDef` gains `params` field |
| `Source/Parser/Modules.rs` | `test_def()` parses optional param list |
| `Source/Sema/Registration.rs` | property-test param validation, E2301 |
| `Source/Sema/CheckerItems.rs` | check_test_def property-test path |
| `Source/Codegen/mod.rs` | `emit_test_harness` property + doctest paths |
| `Source/Codegen/TIR.rs` | relax `tir_covers_test_body` for property test bodies |
| `Source/Syntax.rs` | `TEST_DEFAULT_TRIALS` |
| `Source/lib.rs` or `Source/Doctest.rs` | `extract_doctests` |
| `Source/CmdDevTools.rs` | `--coverage` flag and subprocess glue |
| `Source/main.rs` | wire `--coverage` CLI flag |
| `docs/spec/diagnostics.md` | E2301, E2901 entries |
| `tests/ui/` | e2301_no_generator.txt, e2901_doctest_mismatch.txt |
| `examples/features/test_property.jet` | golden example (I5) |
| `examples/features/test_doctest.jet` | golden example (I5) |

---

## Decision verdict

No decision needed — D-TEST1 and D-TEST4 are both ratified and UNBLOCKED.
