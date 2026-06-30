# Protocol/representation hooks push — Tower #41

**Status:** deciding — 5 ballots pending

## What already exists (do not rebuild)

**Rollback hooks (all 3 layers) — DONE:**
- Layer 1: auto-clone snapshot in `#Transact {}` for any Clone type
- Layer 2: custom `Rollback` trait — `impl Type.Rollback { type Snapshot; fn snapshot; fn restore }`
- Layer 3: `on_rollback(() => { })` hook on transaction handle
- Tests: `tests/rollback.rs` (passing); Example: `examples/features/122_rollback_trait.jet`

**D-ITER1 list adapter family — DONE:**
- `.take`, `.skip`, `.filter`, `.map`, `.fold`, `.flat_map` etc. as built-in methods on `[T]`
- Example: `examples/features/89_iter_adapters.jet`

## What needs building (after ballots)

### Display/Debug split (gated on D-DISPLAY-SHAPE + D-DEBUG-REDACT)

Current `Printable`/`JetShow` auto-derives show for all types. Display/Debug splits this:
- `Display`: explicit `impl Type.Display { fn ???(self) -> String }` — user-facing
- `Debug`: auto-derived for all structs/enums; `{val@Debug}` in interpolation
- `{val}` in interpolation requires Display (breaking change from current Printable default)
- `{val@UNKNOWN}` → E0914 (unknown interpolation selector)

**Migration:** introduce lint warning phase (L0xxx) before hard Display enforcement.

Build order (after D-DISPLAY-SHAPE + D-DEBUG-REDACT):
1. `Syntax.rs`: add `TRAIT_DISPLAY`, `TRAIT_DEBUG`, `INTERP_SELECTOR_DEBUG`
2. `AST.rs` (`StrPart`): add selector enum for `{val@Debug}` / `{val}` 
3. `Traits.rs`: register synthetic Display (no auto-derive) + Debug (auto-derived)
4. `Sema/Diagnostics.rs` `is_printable`: require Display for bare `{}` → E0915
5. `Codegen/Items.rs`: split JetShow — Debug backing (auto) vs Display override (user)
6. Diagnostic codes: E0914 (unknown selector), E0915 (no Display impl), E0916 (Debug blocked by field)
7. Example + UI snapshots

### Iterator/Index/Suffix hooks (gated on D-ITER-HOOK, D-INDEX-HOOK, D-LITSUFFIX-SCOPE)

**Owner direction (2026-06-30):** dual-tier on iterator + index; dot-constructor for units.

- **D-ITER-HOOK (rec A):** `.each`/`.to_list()` for beginners; expert `impl Iterable` + `Iterator` for zero-copy `for x in mytype`
- **D-INDEX-HOOK (rec A):** built-in `[]` for List/Map; `.get`/`.set` for custom types; expert `impl Index`/`IndexMut` for `mytype[k]`
- **D-LITSUFFIX-SCOPE (rec A):** no literal-suffix hook — units construct as `px.{100}` (dot-constructor family, D-DOTCTOR1)

## Ballots needed

- **D-DISPLAY-SHAPE**: method name in `impl Type.Display` — `fn display` vs `fn to_string` vs `fn format`
- **D-DEBUG-REDACT**: field-level marker to exclude from Debug output — `#[Redact]` vs `#[Debug.skip]` vs defer
- **D-ITER-HOOK**: dual-tier iterable — beginner `.each`/`.to_list()` + expert Iterable/Iterator
- **D-INDEX-HOOK**: dual-tier indexing — beginner `.get`/`.set` + expert Index/IndexMut
- **D-LITSUFFIX-SCOPE**: dot-constructor units (`px.{100}`) — close literal-suffix hook

Agent recommendations: Display=A (fn display), Debug=A (#[Redact]), Iterator/Index=A (dual-tier), Units=A (dot-constructor, no suffix hook).

## Acceptance

- Display/Debug example with `impl Type.Display`, `{val}` vs `{val@Debug}`, field redaction
- E0914 (unknown selector), E0915 (no Display) snapshots
- Rollback tests remain green
- Hook set is closed — only Display and Debug unless owner ratifies iterator/index/suffix
