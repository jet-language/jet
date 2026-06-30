# Protocol/representation hooks push — Tower #41

**Status:** implemented 2026-06-30

## What already exists (do not rebuild)

**Rollback hooks (all 3 layers) — DONE:**
- Layer 1: auto-clone snapshot in `#Transact {}` for any Clone type
- Layer 2: custom `Rollback` trait — `impl Type.Rollback { type Snapshot; fn snapshot; fn restore }`
- Layer 3: `on_rollback(() => { })` hook on transaction handle
- Tests: `tests/rollback.rs` (passing); Example: `examples/features/122_rollback_trait.jet`

**D-ITER1 list adapter family — DONE:**
- `.take`, `.skip`, `.filter`, `.map`, `.fold`, `.flat_map` etc. as built-in methods on `[T]`
- Example: `examples/features/89_iter_adapters.jet`

## Shipped (2026-06-30)

### Display/Debug split (D-DISPLAY-SHAPE + D-DEBUG-REDACT + D-DISPLAYDBG2)

- `Display`: explicit `impl Type.Display { fn display(self) -> String }` — user-facing
- `Debug`: auto-derived for structs/enums; `{val@Debug}` in interpolation; `#[Redact]` → `[redacted]`
- `{val}` in interpolation requires Display (L0520 migration lint for auto-printable structs without Display)
- `{val@UNKNOWN}` → E0914

Examples: `179_display_debug.jet`
UI: `debug_unknown_selector` (E0914), `display_no_impl` (L0520 via `// @all_diags`)

### Iterator/Index hooks (D-ITER-HOOK, D-INDEX-HOOK)

- **Iterable/Iterator:** `loop x in mytype` when expert impl present; `.each`/`.to_list()` remain beginner path
- **Index/IndexMut:** `mytype[k]` / assign when expert impl present; List/Map keep native `[]`

Examples: `180_iter_hook.jet`, `181_index_hook.jet`

### Units (D-LITSUFFIX-SCOPE)

- No literal-suffix hook — units construct as `px.{100}` (dot-constructor family, D-DOTCTOR1). Doc-only.

## Follow-ups

- E0916 defined but not yet emitted (non-debuggable field blocks auto-derive)
- Field-of-index assign in trait bodies may copy (181 `set` uses get/mutate/write-back workaround)

## Acceptance ✓

- Display/Debug example with `impl Type.Display`, `{val}` vs `{val@Debug}`, field redaction
- E0914 (unknown selector), E0915 (no Display), L0520 (migration lint) snapshots
- Iterable/Index golden examples pass
- Rollback tests remain green
