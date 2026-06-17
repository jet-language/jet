# Plan: Jet module system

**Status:** design ratified (D-MOD1 A, D-MOD2 A, D-MOD3 A, D-MOD4 pending) — ready to plan, not yet ready to implement.
**Blocks:** multi-file projects beyond the current `use "path" as alias` primitive.
**Depends on:** nothing — builds on existing loader infrastructure.

---

## What we're building

Rust's module system, with two surface differences:

| Rust | Jet |
|---|---|
| `mod math;` | `module math;` |
| `mod math { … }` | `module math { … }` |
| `use math::clamp;` | `use math.clamp;` |
| `use math::{A, B};` | `use math.{A, B};` |
| `pub use math::clamp;` | `pub use math.clamp;` |
| `math::clamp(…)` | `math.clamp(…)` |

Everything else (file resolution, visibility rules, declaration-in-parent requirement, re-export) is identical to Rust.

---

## Existing infrastructure to keep

- `use "path/to/file" as alias` — **keep working**. It already loads a file and makes its public items accessible as `alias.Item`. This is the single-file entry point and must never require ceremony.
- `pub` keyword and cross-file visibility checks in sema — **keep as-is**. They already enforce `pub` across file boundaries; we extend them to inline module boundaries.
- `Item::Module(ModuleDecl)` in ast.rs — this is the **JetOS** module declaration (`module name { sources: …, contributions: … }`). It is **not** a code module. We add a new variant alongside it.

---

## Keyword conflict

`module` (`KW_MODULE`) is already the JetOS declaration keyword. The two uses are syntactically disjoint:

- **JetOS module body:** first real token after `{` is `sources`, `imports`, or an identifier followed by `.` (a contribution path like `env.hostname`).
- **Code module body:** first real token after `{` is `fn`, `struct`, `enum`, `impl`, `pub`, `const`, `use`, another `module`, or `}` (empty).
- **Declaration form:** `module math;` (semicolon, no brace) is always a code module reference — JetOS modules never have this form.

The parser disambiguates by peeking past the `{` before committing to a parse branch. No new keyword needed.

---

## File resolution

`module math;` in `src/main.jet` searches in order:

1. `src/math.jet` — file module
2. `src/math/mod.jet` — directory module

If neither exists, compile error pointing at the `module math;` line. If both exist, compile error (ambiguous).

`module math { … }` is an inline module — no file lookup. Its items live in the `math` namespace of the containing file.

---

## Phases

### Phase 1 — AST + parser

**New AST node** in `src/ast.rs`:

```rust
pub struct CodeModule {
    pub name: String,
    pub name_span: Span,
    /// None = declaration (module math;), Some = inline body (module math { … })
    pub body: Option<Vec<Item>>,
    pub span: Span,
}
```

Add `Item::CodeModule(CodeModule)` alongside the existing `Item::Module(ModuleDecl)` (JetOS).

**Parser changes** in `src/parser.rs`:

In `top_level_item`, when the next token is `KwModule`:
1. Consume `module`.
2. Expect an ident (the module name).
3. Peek the next token:
   - `;` → `CodeModule { body: None }` (declaration form)
   - `{` → peek one token further to determine JetOS vs code:
     - If `sources`, `imports`, or `ident` followed by `.` → existing `module_decl()` path (JetOS)
     - Otherwise → parse items until `}`, return `CodeModule { body: Some(items) }`

**`use` statement extension** in `src/parser.rs`:

Current `import_decl` already handles `use "path"` and `use module_name`. Extend it to also parse:

- `use alias.Item;` — unqualified import of one item
- `use alias.{Item, Item2};` — unqualified group import
- `pub use alias.Item;` — re-export

This requires distinguishing `use alias.Item` (step-2 unqualified import) from `use "path" as alias` (step-1 module import). The disambiguator: if after `use` the token is a string literal → file import; if it's an ident followed by `.` → unqualified item import; if it's an ident not followed by `.` → bare module import (existing).

New `ImportKind` variants:

```rust
/// use alias.Item  or  use alias.{Item, Item2}
Unqualified { module_alias: String, items: Vec<String>, is_reexport: bool },
```

**`syntax.rs`:** `KW_MODULE` is already `"module"`. No change needed.

---

### Phase 2 — Loader / file resolution

**File:** `src/loader.rs`

When the sema or loader encounters `Item::CodeModule { body: None, name }`:

1. Compute the search paths relative to the current file's directory.
2. Try `{dir}/{name}.jet`, then `{dir}/{name}/mod.jet`.
3. On success: load the file, parse it, attach it to the `ProgramBundle` as a new `LoadedModule` with `alias = name`.
4. On failure: emit a new diagnostic (claim a code in `E05xx` block — module errors).

Inline `CodeModule { body: Some(items) }` needs no file I/O — its items are in scope under the `name` namespace within the current module's sema pass.

---

### Phase 3 — Sema: inline module scoping

**File:** `src/sema.rs`

Inline modules (`module math { … }`) need a nested scope. When sema visits `Item::CodeModule { body: Some(items) }`:

1. Push a new module scope with name `math`.
2. Run the sema pass over `items` in that scope.
3. Pop the scope.
4. Register the module name in the parent scope so `math.clamp` resolves.

Path resolution: `math.clamp` in an expression position is currently parsed as a field access (`Expr::Field`). Sema already resolves `alias.FnName` for file imports — extend this to also resolve inline module items via the same path. The disambiguation (module vs value field access) happens in `infer` when the receiver is a known module name.

---

### Phase 4 — Sema: `use alias.Item` unqualified imports

**File:** `src/sema.rs`

When sema encounters `ImportKind::Unqualified { module_alias, items, is_reexport }`:

1. Look up `module_alias` in the current scope (must be a loaded module or inline module).
2. For each item in `items`: resolve it in the module's public surface, then bind it in the current scope under its unqualified name.
3. If `is_reexport`: mark the binding as `pub` so it appears in the current file's exports.

Error cases:
- `module_alias` not found → `E05xx: no module named 'math' in scope`
- item not found in module → `E05xx: 'clamp' is not exported by 'math'`
- item not `pub` → `E05xx: 'clamp' is private in 'math'`

---

### Phase 5 — Visibility enforcement for inline modules

**File:** `src/sema.rs`

The existing cross-file `pub` checks in sema already work when `module_idx` differs. Inline modules get a synthetic `module_idx` so the same checks apply. Items inside `module math { … }` that are not `pub` are inaccessible from outside — same error message as the file-module case.

---

### Phase 6 — Codegen

**File:** `src/codegen.rs`

Inline `module math { fn clamp(…) … }` lowers as: emit all items directly into the Rust output, but mangle their names as `user_math__clamp` (double-underscore separator) to avoid collisions with items of the same name in other modules. The access path `math.clamp` in expressions is already emitted as a field/method lookup — sema will have resolved it to a concrete function name by codegen time.

File-based modules (`module math;`) are already handled by the loader: they become separate `LoadedModule` entries, each compiled and linked. Codegen for those is unchanged.

---

## Diagnostics to register (claim codes)

| Code | Message | Trigger |
|---|---|---|
| E0501 | `module 'math' not found` | `module math;` but neither `math.jet` nor `math/mod.jet` exists |
| E0502 | `module 'math' is ambiguous` | both `math.jet` and `math/mod.jet` exist |
| E0503 | `'clamp' is not exported by 'math'` | `use math.clamp` but `clamp` is private |
| E0504 | `no module named 'math' in scope` | `use math.clamp` but `math` was never declared |
| E0505 | `'math' is not a module` | `use math.clamp` but `math` is a variable |

---

## Tests to write

Each test is an `examples/` file + expected output, enforced by golden tests.

1. **Inline module, basic** — `module math { pub fn double(n: Int) -> Int { return n * 2; } }` in one file; `main` calls `math.double(5)`. Output: `10`.

2. **Inline module, visibility** — inline module with a private helper; calling it from outside the module emits E0503.

3. **File module declaration** — `module math;` in `main.jet`, `math.jet` with `pub fn clamp`. Call `math.clamp`. Output: clamped value.

4. **Directory module** — `module text;` → finds `text/mod.jet` which declares `module wrap;` → finds `text/wrap.jet`. Call `text.wrap`. Output: wrapped string.

5. **`use` unqualified** — `use math.clamp;` after `module math;`, then call `clamp(…)` unqualified.

6. **`use` group** — `use math.{clamp, lerp};`, call both unqualified.

7. **`pub use` re-export** — `text/mod.jet` with `pub use wrap.wrap;`; caller imports `text` and calls `text.wrap` directly.

8. **Missing module error** — `module missing;` with no matching file → E0501.

9. **Wildcard rejected** — `use math.*;` → compile error (wildcards not supported).

---

## What is NOT in scope

- `super.` and `root.` path qualifiers (absolute/parent paths) — defer until a real use case demands them; relative imports cover single-project needs.
- Nested inline modules more than two levels deep — no explicit prohibition, but no tests; keep implementation simple.
- Module-level doc comments — a later quality-of-life pass.
- LSP auto-import quick-fix (owner-requested in D-MOD3 comment) — separate LSP roadmap item; doesn't block the language feature.
