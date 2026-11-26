# Design Review: Fields Shared by Every Case of an Enum (Proposal Item P19)

**Date:** 2026-11-26  
**Scope:** Shared header fields declared at the top of an `enum` body, fixed-offset single-load field reads and COW-safe writes, construction, pattern matching, ownership, derives, serialization, reflection, formatting, and I9 parity across `O0`, `O1`, `O2`, comptime, and Web.  
**Verdict:** **NEEDS BALLOT** (4 concrete surface/semantics questions require owner resolution before build; the core flat-header layout and single-load mechanism hold strongly).

---

## 1. Problem Statement & Proposal Summary

Proposal item P19 (`Docs/Proposals/2026-11-24-greenfield-language.md`, Part 2 and Part 3.4) adds **shared fields** declared at the top of an `enum` body so data carried by every variant—timestamps on events, source spans and node IDs on AST nodes, senders on protocol messages—is declared once, stored at a fixed byte offset immediately after the discriminant tag, and read or written directly on an enum value without matching its case:

```jet
Event: enum {
    at: Time,
    source: DeviceID,
    Click(x: Int, y: Int),
    Key(code: Int),
    Scroll(dy: F64),
}

^e: Event.Click{at: now(), source: mouse, x: 10, y: 20}
print(e.at)                      // single load at fixed offset; no tag test, no branch
e.at = e.at + 5ms                // writable when `e` is mutable (COW-unique box)
if e == {
    .Click{x, y} -> press(x, y)
    .Key{at, code} -> type(at, code)
    .Scroll{dy} -> scroll(dy)
}
```

*(Note the syntax difference from the proposal draft: live Jet declares variant payloads with parentheses `Click(x: Int, y: Int)` because `Name { ... }` is already a variant group `EnumGroup`, and marks mutable locals `^e: ...`. See Ballot Question 1 in Section 5.)*

Without shared fields, Jet code chooses between two clumsy workarounds:
1. Repeating the fields in every variant (`AST.Expr` repeats `span: Span, id: Int` across ~45 variants today in `Compiler/JetFoundation/Source/AST/Expressions.jet`) and reading them through an exhaustive switch table.
2. Wrapping the enum in an outer struct (`Event: struct { at: Time, kind: EventKind }`), forcing `.kind` indirection on every match and constructor.

---

## 2. Strongest Existing Solutions & Contrast with Jet

| Language / Solution | Mechanism & Primary Source | Trade-Off vs. Jet's Flat Header Box |
|---|---|---|
| **Ada discriminated records with variant parts** | Common components appear before `case Disc is when ...` in a single flat record layout (*Ada 2022 Reference Manual*, §3.8.1 "Variant Parts and Discrete Choices"). | **Closest memory-layout ancestor.** Common fields sit at fixed offsets independent of the discriminant, so reads compile to a single load with zero branch. Ada allows discriminant mutation that re-tags a record in place (requiring runtime constraint checks); Jet keeps whole-value replacement for re-tagging and plain fixed-offset mutation for shared fields. |
| **Kotlin `sealed class` / `sealed interface`** | Base class declares common `val`/`var` properties; subclasses carry case-specific fields (*Kotlin Language Specification*, §"Sealed classes and interfaces"). | Every instance is a GC heap object with class-header + vtable overhead and an inheritance hierarchy. Jet avoids inheritance and virtual dispatch: one flat counted box, tag word at byte 0, shared fields at byte 8+, variant fields immediately after. |
| **Scala 3 parameterized `enum`** | `enum Event(val at: Time)` lifts shared constructor arguments into the generated abstract superclass (*Scala 3 Reference*, "Enumerations"). | Clean surface syntax, but implemented as JVM class inheritance with boxed subclass instances, constructor boilerplate (`extends Event(at)`), and JVM GC. |
| **Dart 3 `sealed class`** | Sealed abstract base class with final/mutable fields and exhaustive `switch` patterns (*Dart Language Specification*, Dart 3.0 Patterns & Class Modifiers). | Same OOP inheritance hierarchy and GC object-header cost as Kotlin/Scala. |
| **TypeScript discriminated unions** | `type Event = { at: Time; kind: "click"; x: number } \| ...` allows `e.at` directly without narrowing (*TypeScript Handbook*, "Narrowing — Discriminated unions"). | Ergonomic read access without a match, but relies on dynamic JS object shapes (hidden classes) with no guaranteed fixed byte offset across variants unless all shapes align manually. |
| **Rust `enum` + wrapper `struct`** | Rust has no shared enum fields; idioms repeat fields per variant or wrap `struct Event { at: Time, kind: EventKind }`. RFC 2593 ("Enum variant types", postponed) and `#[repr(C)]` RFC 2195 discuss representation guarantees, but Rust's default `repr(Rust)` deliberately permits niche packing and per-variant field reordering so common-offset access is not guaranteed without an outer struct. | Wrapper struct gives single-load access at the cost of `.kind` ceremony everywhere; repeating fields loses single-load access and forces a match per read. |
| **Zig `union(enum)` / Odin `union`** | Tagged unions carry variant payloads only (*Zig Language Reference*, "Tagged union"; *Odin Overview*, "Unions"). Common fields require an outer `struct`. | Same `.kind`/`.data` wrapper indirection as Rust. |

**Why Jet's layout fits its existing runtime cleanly:** Jet already represents every payload-bearing enum as a counted heap box (`IRTypeKind.Pointer` with `ir_mark_box` in `Compiler/JetIR/Source/BuildTypes.jet:589-597`) whose discriminant word lives at byte `0` and whose variant members are placed sequentially from byte `8` via `jet_target_layout_place(sizes, aligns, 8, false)` (`Compiler/JetIR/Source/BuildTypes.jet:4366-4416`). Placing shared fields first from byte `8` before each variant's payload gives **exact Ada-grade fixed offsets** (one load, zero tag test, zero branch) while preserving Jet's variant-sized box allocation (`ir_enum_variant_box_size`), COW sharing (`#box.share` / `#box.unique`), and I9 parity across native x86-64, comptime, and WebAssembly.

---

## 3. Complete Compiler & Tooling Inventory (Where Enums Live Today)

Every location in the live repository that defines, parses, checks, lowers, reflects, serializes, formats, or documents enums:

| Layer | Path | Current Behavior / Assumption | Required Change for Shared Fields |
|---|---|---|---|
| **Syntax registry** | `Compiler/JetFoundation/Data/Syntax/Spellings.jet:19` | Documents `Name: enum { A, B(T) }` as cases only. | Document shared fields at the top of an `enum` body. |
| **AST** | `Compiler/JetFoundation/Source/AST/Items.jet:1276-1329, 1626` | `EnumDef` holds `variants: [Variant]` and no `fields: [Field]`. | Add `fields: [Field]` to `EnumDef` (reusing `Field` for visibility, defaults, computed fields, `#Redact`, serde markers) and initialize in `ast_enum`. |
| **AST walks** | `Compiler/JetFoundation/Source/AST/Walk.jet`, `AST/Comptime.jet`, `AST/Nodes.jet` | Walk `EnumDef.variants`, `methods`, `trait_impls`, not `fields`. | Walk `EnumDef.fields` (default/computed expressions and field markers). |
| **Typed facts** | `Compiler/JetFoundation/Source/Facts/Declarations.jet:208-214` | `TTypeDefKind.Enum(variants: [TVariant], methods: [String])` carries no shared `fields: [TField]`. | Extend `TTypeDefKind.Enum(fields: [TField], variants: [TVariant], methods: [String])` (or store shared `TField` list on `Enum`). |
| **Target layout** | `Compiler/JetFoundation/Source/Target/Layout.jet:710-845` | `jet_target_layout_native_enum_facts` and `jet_target_layout_enum_facts_inner_named` place only variant payloads from byte `8`; `.Unit` reports offset `0`, size `8`. | Place shared fields first from byte `8` (native) or after `tag_layout` (C-tag), report shared field offsets in `LayoutFacts.fields`, and place each variant's payload after the shared header. |
| **Parser** | `Compiler/JetParser/Source/Parser/Items.jet:2315-2497` | `parser_parse_enum` parses only markers, `state`, methods, `impl`, and variants/groups; `Name { ... }` is an `EnumGroup`. | Parse leading `[priv] [markers] name: Type[{default}] [-> expr],` fields before variants (distinguished unambiguously by `:` after name, since `name: fn` is already rewritten to `TOKEN_KW_FN`); reject shared fields written after variants with a clear fix. |
| **Formatter** | `Compiler/JetParser/Source/Formatter/Items.jet:2668-2760` | `jfmt_fmt_enum` passes `fields: [Field]{}` (line 2755) and collapses `cases_only` onto one line. | Format `e.fields` at the top of the enum body before variants/groups; disable one-line collapse when `e.fields.len() > 0`. |
| **Registration & checks** | `Compiler/JetSema/Source/Sema/Registration/Items.jet:1933, 2774`, `Checked.jet`, `EnumNumbers.jet`, `SharedCycles.jet`, `PublicAPI.jet` | Registers only `EnumDef.variants`; `EnumNumbers.jet` checks only variant payloads for `E1120`; cycle detection walks variant payloads only. | Register shared field symbols; reject duplicate shared fields, shared-vs-variant field collisions, shared fields on `#Layout(U8)`/`#Specified` (`E1120`) and `impl Effect` enums; include shared fields in recursive-edge cycle boxing (`boxed_edges`) and `PublicAPI.jet`. |
| **Field read & write** | `Compiler/JetSema/Source/Sema/Expressions/Places.jet:432-505, 695-908`, `Statements/Assignments.jet`, `Statements.jet` | `sema_expression_field_checked` resolves stored fields only on `StructDef` (line 503); `Assignments.jet` checks field writes only on structs. | Resolve shared fields on `EnumDef` (with generic substitution, `priv` visibility, `#Secret` taint, computed fields, `#As`, `Pin<T>`, `@T`, `?.field`, and field-vs-method collision checks); allow `^e.shared = v` and compound assignments on mutable enum places. |
| **Constructors & ctor values** | `Compiler/JetSema/Source/Sema/Expressions/Constructors.jet:1080-1369` | Checks `EnumLit` against `variant.payload` only; `sema_expression_enum_constructor_value` builds `fn` signature from `variant.payload` only; `.Unit` rejects any args. | Include shared fields (and their defaults) in `EnumLit` checking, generic type-arg inference, move recording, and `SemaEnumConstructorSignature` (including `^^` take conventions for non-copy shared fields). |
| **Patterns & coverage** | `Compiler/JetSema/Source/Sema/Patterns.jet:658-995`, `Coverage.jet:130` | `sema_pattern_place_named_slots` requires every field or `..` (`E0326`/`E0327`) and checks only `variant.payload`. | Allow binding shared fields in `.Case{at, ...}` (including unit cases `.Quit{at}`), apply the balloted `E0326`/`E0327` and positional-pattern rule, and keep exhaustiveness/coverage sound. |
| **Ownership & footprints** | `Compiler/JetSema/Source/Sema/Ownership/` (`Crossings.jet`, `CopyClassify.jet`, `WindowFacts.jet`, `CallMarks.jet`, `Checks.jet`) | Walks enum variant payloads for `Send`, `Copy`/`Clone`, `Close`, `#MoveOnly`, and `#SingleUse`; checks `^self.field` footprints on structs. | Include `EnumDef.fields` in every ownership/send/copy/close traversal; allow `^self.shared_field` method footprints; **forbid partial `.move()` out of an enum's non-copy shared field** (counted box cannot track per-field move drop flags). |
| **Derives & prelude templates** | `Compiler/Prelude/Derives.jet:19-152`, `Compiler/JetSema/Source/Sema/Registration/Derives.jet:2338-2363, 2509-3732` | `sema_derive_enum_type_info` passes empty `[CtValue]{}` for `fields`; `Equatable.Derive` and `Comparable.Derive` iterate only `cases` and `left.reflect().fields`; readiness checks inspect only variant payloads. | Populate `TypeInfo.fields` with shared fields; update `Equatable.Derive` to compare shared fields first (branch-free short-circuit!) and `Comparable.Derive` to compare shared fields in order; include shared field types in derive readiness/auto-derive checks. |
| **Serde** | `Compiler/JetSema/Source/Sema/Registration/SerdeItems.jet:480-520, 1478-1740` | Generates `encode`, `decode`, and streaming `read` from `variant.payload` only; `sema_serde_items_enum_wire` collects only variant types. | Include shared field types in `sema_serde_items_enum_wire`, serialize/deserialize shared fields in all 3 tagging modes per Ballot Question 4, honoring `#Rename` and field defaults. |
| **Comptime folding & reflection** | `Compiler/JetSema/Source/Sema/Comptime/Fold.jet:171-210`, `Comptime/Reflection.jet:2757, 2827-2845, 3271-3336` | `Fold.jet:171` folds `.Field` on `.Struct` only; `Reflection.jet:3324` passes empty `fields` for enums; `specialize_enum` specializes variants only. | Fold `.Field` on `TComptimeValue.Enum(_, _, args)`; populate `TypeInfo.fields` with shared `FieldInfo` rows (with layout offsets); specialize shared fields in `sema_comptime_reflection_specialize_enum`. |
| **IR layout, ownership & lowering** | `Compiler/JetIR/Source/BuildTypes.jet:580-598, 2363-2402, 4350-4509`, `BuildExpr.jet:2447-2500, 4912-4990, 5497-5680, 6412-6465, 8561-8593, 9568-9597`, `BuildOwn.jet:225-620`, `BuildStatic.jet:673, 1092`, `BuildRoutes.jet:6376` | Enum with all `.Unit` variants becomes `IRTypeKind.Word`; `ir_variant_members_laid` places only variant fields from byte `8`; `.Field` read/write assumes `.Aggregate`; `ir_render_nominal` prints `.Unit` without fields; `ir_comptime_enum` maps variant payload only. | Classify any enum with non-computed shared fields as counted heap box (`IRTypeKind.Pointer` + `ir_mark_box`); prepend shared fields in `ir_variant_members_laid` from byte `8`; lower `.Field` reads on enum boxes via single `IROp.Load` at fixed offset; lower `.Field` writes through `ir_window_subject` (`ir_box_make_unique`) + `IROp.Offset`; update `ir_render_nominal` (`.Unit` with `members.len() > 0` uses `ir_render_record`), `BuildStatic.jet`, `BuildExpr.jet` pattern slot indexing, and `BuildRoutes.jet` runtime reflection capture. |
| **Native backend & DWARF** | `Compiler/JetBackend/Source/Image/Dwarf.jet:735-747`, `BuildOwn.jet:353-381` | `ir_debug_variants_of` reads `ir_variant_members` directly; `Dwarf.jet` emits `DW_TAG_variant_part`. | Shared fields flow into `ir_debug_variants_of` automatically; emit shared fields as top-level `DW_TAG_member` entries before `DW_TAG_variant_part` so `jet debug` inspects `e.at` directly regardless of active variant. |
| **Web (WASM) backend** | `Compiler/JetWeb/Source/Web/Emit.jet`, `Select.jet`, `Leaf.jet` | Lowers `IRModule` directly (no second codegen; enum boxes are `i64` pointers into linear memory). | **Automatic I9 parity** once `JetIR` lowers shared-field reads/writes cleanly; add Web execution proof in tests. |
| **Editors & docs** | `Tools/Editors/TreeSitter/grammar.js:235`, `Tools/Editors/jet.tmGrammar`, `Docs/Spec/spec.md`, `Docs/Spec/Reference/metaprogramming.md:559`, `Docs/Spec/Reference/syntax-surface.jet` | `metaprogramming.md:559` explicitly states *"An enum has no stored fields"*. | Verify TreeSitter parses shared fields in `enum_def` with zero ERROR/MISSING nodes; update `spec.md`, `metaprogramming.md`, and `syntax-surface.jet`. |

---

## 4. Adversarial Attacks & Edge-Case Analysis

### Attack 1: Unit-Only Enums with Shared Fields & Static Singleton Sharing
- **Claim attacked:** "Just add shared fields to payload enums."
- **Failure mode:** Look at `BuildTypes.jet:589-597`:
  ```jet
  loop variant in variants {
      if variant.payload != .Unit {
          boxed: ir_intern_type(m, "enum:{identity}", ...)
          return Val(ir_mark_box(m, boxed))
      }
  }
  return Val(ir_intern_type(m, "enum:{identity}", ir_layout(IRTypeKind.Word, ...)))
  ```
  If an enum has shared fields and **all** its variants are `.Unit` (e.g., `Signal: enum { at: Int, Start, Stop }`), `BuildTypes.jet:589` would silently lower `Signal` to an 8-byte `IRTypeKind.Word` with **nowhere to store `at`**! Similarly, `BuildMaps.jet:8` hashes `.Word` enums by tag word alone, and `BuildExpr.jet:4661` compares `.Word` enums by tag word alone.
- **Resolution:** Classify an enum as a counted box (`IRTypeKind.Pointer` + `ir_mark_box`) whenever `shared_stored_fields.len() > 0 || any_variant_has_payload`. Because `ir_variant_members` prepends shared fields, `members.len() > 0` for every variant of `Signal`, which also prevents `BuildExpr.jet:6424` (`if payload == .Unit && members.len() == 0`) from incorrectly folding `Signal.Start{at: x}` into a static singleton!

### Attack 2: Alignment, Padding, Outsized Structs, and Variant-Sized Boxes
- **Claim attacked:** "A shared field sits at the same offset in every case, right after the tag, so `e.at` is one load with no branch."
- **Failure mode:** Could a high-alignment variant field (e.g. a 16-byte SIMD vector or `#Layout(c, align(16))` struct) shift a shared field's offset, or could `ir_variant_member_outsized` / `boxed_edges` wrap a shared field in one variant and leave it inline in another?
- **Proof of safety:**
  1. `jet_target_layout_place(sizes, aligns, 8, false)` (`BuildTypes.jet:4412`) places members strictly in **written order** starting at byte `8` without reordering. Because every variant places `[shared_fields..., variant_fields...]` in that exact prefix order, shared field `k` gets the exact same byte offset `>= 8` in every variant; higher-alignment variant fields add padding *only after* the shared header.
  2. `ir_variant_member_outsized` (`BuildTypes.jet:4431`) checks only `m.types[ty].size > JET_TARGET_LAYOUT_VARIANT_INLINE_BYTES` and properties of `definition` (not the variant), so every variant agrees on `boxed`.
  3. **Cycle boxing fix required:** For a recursive shared field (`next: Event`), `definition.boxed_edges` must record one shared edge key (e.g., `"__shared.{field.name}"`) checked identically in every variant of `ir_variant_members_laid`—never per-variant edge names that could diverge.
  4. **Variant-sized boxes (`BuildTypes.jet:4480`):** Because every variant's `members` starts with the shared header, `ir_enum_variant_box_size` allocates at least `ir_heap_block_fill(ir_align8(shared_end))` for `.Unit` cases while still shrinking small cases relative to wide cases.

### Attack 3: COW / Refcount Corruption on Shared-Field Mutation (`^e.at = ...`)
- **Claim attacked:** Proposal Part 3.4 writes `e.at = e.at + 5ms // writable, if e is`.
- **CRITICAL SOUNDNESS CATCH:** In Jet, payload enums are **counted heap boxes** copied by refcount increment (`#box.share`, `BuildOwn.jet:225-245`), NOT deep-copied stack aggregates. Look at `BuildExpr.jet:4966-4990` (`ir_place_address_in`):
  - Currently `.Field` write lowering checks `if ir_kind_of(m, b, base) != .Aggregate -> return ir_fail_slot(...)`.
  - If an implementation naively wrote through `base + member.offset` on an enum box pointer without making the box unique, then:
    ```jet
    ^a: Event.Click{at: 1, x: 0, y: 0}
    b: a.copy()
    a.at = 99
    assert_eq(b.at, 1) // WOULD FAIL (b.at == 99) — SILENT ALIAS CORRUPTION!
    ```
    This would violate **I1** (value semantics / memory safety) and **data-race freedom** if `b` crossed a task boundary!
- **Resolution:** In `BuildExpr.jet:4966` (`ir_place_address_in`), when `receiver` is a payload enum box (`ir_window_box`), acquire the unique box via `ir_window_subject(m, p, b, receiver, receiver_ty, span)` (`BuildExpr.jet:8572`, which calls `ir_box_make_unique` and stores the unique box pointer back to `receiver`!), then apply `ir_wrapper_unique` if `member.boxed`, then `IROp.Offset` at `member.offset`. Also return `true` in `ir_place_needs_address` (`BuildExpr.jet:4921`) for enum shared fields. This reuses the exact proven COW-unique mechanism already used by `if e.in_place() == .Click(x, y)`.

### Attack 4: Partial Moves Out of a Shared Field (`e.name.move()`)
- **Claim attacked:** "Shared fields behave just like struct fields."
- **CRITICAL SOUNDNESS CATCH:** For a stack-allocated struct, Jet's ownership checker allows partial moves (`s.name.move()`) and suppresses dropping `s.name` individually when `s` leaves scope. An enum is a **counted heap box** whose destructor `ir_box_drop` (`BuildOwn.jet:510-531`) drops **every** member in `ir_box_shapes` unconditionally when the refcount hits 0—there are no per-field runtime drop flags inside an enum box!
- **Resolution:** Sema ownership checking (`Sema/Ownership/`) MUST reject moving a non-copy shared field out of an enum place (`e.name.move()`, passing `e.name` to a `^^T` parameter) with a clear diagnostic pointing to `e.name.copy()` or whole-enum pattern destructuring—unless/until box partial-move zeroing is explicitly designed. Reading/borrowing `e.name`, mutating `^e.name = new_name` (which drops old `e.name` and writes `new_name`), and moving the whole enum `e` remain fully safe.

### Attack 5: `Equatable.Derive` & `Comparable.Derive` Silent Semantic Bugs
- **Claim attacked:** "Derived traits automatically work on enums."
- **Failure mode:** Look at `Compiler/Prelude/Derives.jet:26-50` and `:82-122`:
  - `Equatable.Derive` iterates ONLY `target.reflect().cases` and `left.reflect().fields`.
  - If `TypeInfo.fields` holds shared fields while `CaseInfo.payload` holds variant-specific fields (the clean non-duplicating design), an unupdated `Equatable.Derive` would **completely ignore shared fields**, making `Event.Click{at: 1, x: 0, y: 0} == Event.Click{at: 999, x: 0, y: 0}` evaluate to **`true`**!
  - Likewise, `Comparable.Derive` would silently ignore shared fields, and auto-derive readiness checks (`Registration/Derives.jet:2509-3732`) would auto-derive `Equatable`/`Comparable` even when a shared field is NOT `Equatable`/`Comparable`!
- **Resolution:**
  1. Update `Equatable.Derive` to compare `#loop field in target.reflect().fields` **first** (zero-branch short-circuit!) before matching variants.
  2. Update `Comparable.Derive` to compare variant ordinal first, then `#loop field in target.reflect().fields` (with IEEE `totalOrder` for `Float`/`F32` and recursive `.compare` for self-type), then variant fields.
  3. Update `Registration/Derives.jet` readiness and auto-derive checks to require every shared field's type to satisfy the derived trait.

### Attack 6: Printing (`Debug` / `Print`), DWARF, and Runtime Reflection (`reflect.capture`)
- **Failure mode 1 (`BuildTypes.jet:2381-2394`):** `ir_render_nominal` switches on `variant.payload`:
  - `.Unit` prints only `variant.name`, silently dropping all shared fields!
  - `.Single` indexes `members[0]`, which would print the **first shared field** instead of the single payload if `.Single` were allowed!
  - Fix: When `members.len() > 0` on `.Unit` (or `.Named`), render via `ir_render_record(m, p, b, builder, slot, variant.name, members, mode, span)` (e.g., `Quit { at: 10 }`, `Click { at: 10, x: 1, y: 2 }`), which also automatically honors `#Redact` on shared fields!
- **Failure mode 2 (`BuildRoutes.jet:6376`):** Runtime reflection `reflect.capture(e)` must include shared fields on `Value` so runtime inspection matches compile-time `T.reflect().fields`.
- **Failure mode 3 (`Dwarf.jet:735-747`):** Emit shared fields as top-level `DW_TAG_member` siblings of the discriminant before `DW_TAG_variant_part` so `jet debug` displays `e.at` without requiring variant dispatch.

### Attack 7: Field Name Collisions, Visibility, Defaults, Computed Fields, and Markers
- **Collisions (settled — compile errors):**
  - Duplicate shared field names -> `E0303` / duplicate field error (same as struct).
  - Shared field name colliding with any variant payload field name -> compile error at the colliding variant field (already settled in `Docs/Audits/2026-11-24-greenfield-language-review.md`, C31).
  - Shared field name colliding with an enum method name -> existing field-vs-method collision diagnostic (`Spellings.jet:67`).
  - Shared field written *after* a variant -> parse error directing the author to place shared fields above the cases.
- **Visibility (`priv`):** Supported via `Field.is_pub` / `is_package_pub`; a `priv` shared field without a default makes the enum unconstructible outside its module (identical to structs).
- **Defaults (`retries: Int{0}`):** Supported via `Field.default`; omitted defaulted shared fields fill automatically in `Event.Click{x: 1, y: 2}` and `Event.Quit{}` (and in Serde `Decode`).
- **Computed fields (`name: T -> expr`):** Supported identically to structs (zero storage, skipped in `ir_variant_members_laid`, desugared in `Places.jet:853`).
- **Markers on enums & shared fields:**
  - `#Layout(U8)` / `#Specified` (`EnumNumbers.jet`): refuse shared fields with `E1120` (integer backing cannot store fields).
  - `#Layout(c)`: update `Target/Layout.jet:710-781` to lay out `{ tag, shared..., union { variant_payloads } }` (note `BuildForeign.jet:596` already forbids passing Jet enums by value across C FFI).
  - `#Layout(columnar)`: already struct-only (`Parser/Items.jet:2370`); no change needed now (if columnar enums are added later, shared fields become top-level columns).
  - `impl Effect`: refuse shared fields on effect declaration enums.
  - `state { }` on enums: works naturally—shared fields persist across enum state transitions.
  - `#Open`: adding a new variant never changes shared field offsets; adding a required shared field is a public API breaking change tracked in `PublicAPI.jet`.
  - `#Redact` and `#As` on shared fields: reuse existing struct field handling (`E1129`/`E1131` for `#As`).

### Attack 8: Generic Enums with Shared Fields & Constructor-as-Function-Value
- **Generic inference:** Given `Node<T>: enum { tag: T, Leaf, Branch(left: Node<T>, right: Node<T>) }`, constructing `Node.Leaf{tag: 42}` MUST infer `T = Int` from the shared field argument `tag`, not only from variant payload arguments (`Constructors.jet`).
- **Constructor-as-function-value (`Constructors.jet:1215-1326`):** Reading `Event.Click` or `Event.Quit` without calling it produces `fn(*, at: Time, x: Int, y: Int) -> Event` and `fn(*, at: Time) -> Event` (respecting `^^` move marks on non-copy shared fields, `E0227`, and omitting defaulted shared fields? Note: in Jet, a constructor function value lists all declared slots without defaults, or matches struct/variant callable rules—specify clearly in implementation). If a unit variant has required shared fields, bare `Event.Quit` in non-function position reports missing shared fields rather than constructing an uninitialized box!

### Attack 9: Comptime Constant Folding & Static Blobs (`Fold.jet`, `BuildStatic.jet`, `BuildExpr.jet:9568`)
- `Comptime/Fold.jet:171` must fold `.Field` reads on `TComptimeValue.Enum(_, _, args)` by matching `TComptimeEnumArg.name`.
- `BuildStatic.jet:673, 1092` and `BuildExpr.jet:9568` (`ir_comptime_enum`) must lay out named shared + variant args from `TComptimeValue.Enum` into the static/runtime box using `ir_variant_members`, ensuring `#run` / `const` enum values match runtime layout byte-for-byte across all tiers (I9).

---

## 5. Open Surface Questions Requiring Owner Ballot (`NEEDS BALLOT`)

Before any build commit starts on `#7017`, the following **four** surface/semantics choices need owner ratification (can be bundled as one ballot with four sub-choices):

### Q1. Variant payload declaration syntax vs. existing `EnumGroup`
- **Conflict:** Live Jet declares named variant payloads with parentheses (`Click(x: Int, y: Int)`, `Parser/Items.jet:2426-2496`) and reserves `Name { ... }` inside an enum body for **variant groups** (`EnumGroup`, lines 2445-2473). Proposal P19 drafted `Click{x: Int, y: Int}`.
- **Options:**
  - **A (Recommended):** Keep live Jet's parenthesized case declarations (`Event: enum { at: Time, Click(x: Int, y: Int), Quit }`) alongside brace construction (`Event.Click{at: t, x: 1, y: 2}`) and brace patterns (`.Click{at, x, y}`). Zero conflict with `EnumGroup`, zero parser ambiguity, zero migration churn.
  - **B:** Allow brace-delimited payload declarations (`Click{x: Int, y: Int}`) disambiguated from `EnumGroup` by `ident :` lookahead inside `{ ... }`.

### Q2. Single positional variants (`Key(Int)`) when an enum has shared fields
- **Conflict:** Jet forbids mixing positional and named arguments in `EnumLit` (`E0764`) and forbids multi-value positional variants (`Parser/Items.jet:2494`). On an enum with shared field `at: Time`, a unit case constructs cleanly as `Event.Quit{at: t}`, but a single positional case `Key(Int)` has no field name for its payload.
- **Options:**
  - **A (Recommended):** On an enum with shared fields, allow unit cases (`Quit`) and named-field cases (`Key(code: Int)`), and refuse single positional cases (`Key(Int)`) at declaration time with a diagnostic whose fix names the payload (`Key(value: Int)`). Every constructor and wire encoding is uniformly named.
  - **B:** Allow mixed positional + named arguments (`Event.Key(65, at: t)`) and mixed patterns (`.Key(code, at: t)`), adding a new mixed-arg grammar/formatter/sema form.

### Q3. Pattern matching rules (`E0326` / `E0327` and positional `.Click(x, y)`)
- **Conflict:** Today (`Patterns.jet:839-850`), `.Click{x, y}` must name every field or end with `, ..` (`E0326`), and `, ..` is an error (`E0327`) when all fields are named. Proposal Part 3.4 (`2026-11-24-greenfield-language.md:941-951`) writes `.Click{x, y}` (omitting shared fields `at`, `source` **without** `..`) alongside `.Key{at, code}`. Also, live Jet allows positional patterns `.Click(x, y)` on named-field variants (`Patterns.jet:960-995`).
- **Options:**
  - **A (Recommended — matches Proposal Part 3.4):** Shared fields are **opt-in** in case patterns: `.Click{x, y}` checks `E0326` against `Click`'s variant-specific fields only (no `..` required to omit shared fields, since `e.at` is readable outside the match); `.Key{at, code}` and `.Quit{at}` bind shared fields explicitly; `.Quit` matches without braces; `..` is redundant (`E0327`) only when every variant-specific field is named (unless a shared field was explicitly skipped? Clarify: allow `.Click{x, ..}` when omitting `y`, forbid `.Click{x, y, ..}` under `E0327`). Positional `.Click(x, y)` binds variant-specific fields only (making migration of enums like `AST.Expr` frictionless).
  - **B (Strict uniform `E0326`):** Shared fields belong to every case's required pattern field set, so omitting `at` requires `.Click{x, y, ..}` (and `.Quit` requires `.Quit{..}` unless bare `.Quit` is special-cased). Positional `.Click(x, y)` either binds `[shared..., variant...]` or is refused when shared fields exist.

### Q4. Default externally-tagged Serde wire shape
- **Conflict:** Today (`SerdeItems.jet:1532-1561`), externally-tagged enums encode `Click(x: Int, y: Int)` as `{"Click": {"x": 10, "y": 20}}` and unit `Quit` as `"Quit"`.
- **Options:**
  - **A (Recommended):** Put shared fields inside the variant object for externally-tagged enums (`{"Click": {"at": ..., "x": 10, "y": 20}}`, `{"Quit": {"at": ...}}`), alongside the tag for `#Tag("type")` (`{"type": "Click", "at": ..., "x": 10, "y": 20}`), and flat for `#Untagged` (`{"at": ..., "x": 10, "y": 20}`). Symmetric with construction, zero invented keys.
  - **B:** Reject externally-tagged Serde on enums with shared fields unless `#Tag("...")` is specified.

---

## 6. Binding Cutover List

Once the ballot resolves Q1–Q4, `#7017` cannot close until **every** item below is delivered and proven:

### A. Compiler & Prelude Implementation
1. **AST & Facts:**
   - `Compiler/JetFoundation/Source/AST/Items.jet`: add `fields: [Field]` to `EnumDef` and `ast_enum`.
   - `Compiler/JetFoundation/Source/AST/Walk.jet`, `AST/Comptime.jet`: walk `EnumDef.fields`.
   - `Compiler/JetFoundation/Source/Facts/Declarations.jet`: add `fields: [TField]` to `TTypeDefKind.Enum`.
   - `Compiler/JetFoundation/Source/Target/Layout.jet`: update `jet_target_layout_native_enum_facts` and `jet_target_layout_enum_facts_inner_named` to place shared fields first from byte `8` (or after C tag) and report shared field offsets in `LayoutFacts.fields`.
2. **Parser & Formatter:**
   - `Compiler/JetParser/Source/Parser/Items.jet`: parse shared fields before variants in `parser_parse_enum`; reject shared fields after variants; enforce Q1 & Q2 declaration rules.
   - `Compiler/JetParser/Source/Formatter/Items.jet`: format `e.fields` in `jfmt_fmt_enum` and disable `cases_only` one-line collapse when shared fields exist.
3. **Sema:**
   - `Compiler/JetSema/Source/Sema/Registration/Items.jet`, `Checked.jet`, `EnumNumbers.jet`, `SharedCycles.jet`, `PublicAPI.jet`: register shared fields, enforce duplicate/collision/`E1120`/`Effect` checks, cycle boxing with unified shared edge keys, and public API snapshots.
   - `Compiler/JetSema/Source/Sema/Expressions/Places.jet`, `Statements/Assignments.jet`, `Statements.jet`: field reads (`e.field`, `e?.field`, generic substitution, `priv`, `#Secret`, `#As`, computed fields, field-vs-method collisions) and field writes (`^e.field = v`, compound assignment) on enums.
   - `Compiler/JetSema/Source/Sema/Expressions/Constructors.jet`: shared fields (and defaults) in `EnumLit`, generic inference, move checking, and `SemaEnumConstructorSignature`.
   - `Compiler/JetSema/Source/Sema/Patterns.jet`, `Coverage.jet`: shared field binding in `.Case{...}` (including unit cases) and balloted Q3 `E0326`/`E0327`/positional rules.
   - `Compiler/JetSema/Source/Sema/Ownership/`: include shared fields in `Send`/`Copy`/`Clone`/`Close`/`#MoveOnly`/`#SingleUse`, allow `^self.shared` footprints, and reject partial `.move()` out of non-copy shared fields.
   - `Compiler/Prelude/Derives.jet` & `Compiler/JetSema/Source/Sema/Registration/Derives.jet`: populate `TypeInfo.fields`, compare shared fields in `Equatable.Derive` and `Comparable.Derive`, and check shared field readiness for derives.
   - `Compiler/JetSema/Source/Sema/Registration/SerdeItems.jet`: encode, decode, and streaming read for shared fields per Q4 (`#Rename`, defaults, all 3 tag modes).
   - `Compiler/JetSema/Source/Sema/Comptime/Fold.jet`, `Comptime/Reflection.jet`: fold `.Field` on `TComptimeValue.Enum`, populate `TypeInfo.fields` and layout offsets, specialize generic shared fields.
   - `Compiler/Prelude/Diagnostics.jet`: register any new diagnostic codes (what/why/fix + safety class + UI snapshots, I4).
4. **IR, Backend & Web:**
   - `Compiler/JetIR/Source/BuildTypes.jet`: classify shared-field enums as counted boxes (never `Word`); prepend shared fields in `ir_variant_members_laid`; update `ir_render_nominal` (`.Unit` with shared fields + `#Redact`).
   - `Compiler/JetIR/Source/BuildExpr.jet`: single-load `.Field` read on enum boxes (`ir_members_at`/`ir_member_of`); COW-unique `.Field` write via `ir_window_subject` in `ir_place_address_in` + `ir_place_needs_address`; pattern slot indexing past shared prefix; `ir_comptime_enum` named shared+variant lowering.
   - `Compiler/JetIR/Source/BuildStatic.jet`: static enum blobs with shared fields.
   - `Compiler/JetIR/Source/BuildRoutes.jet`: runtime `reflect.capture` exposing shared fields.
   - `Compiler/JetBackend/Source/Image/Dwarf.jet` & `BuildOwn.jet`: emit shared fields as top-level `DW_TAG_member` entries before `DW_TAG_variant_part`.

### B. Verification, Proofs, Docs & Tooling
5. **Single-load codegen proof:** Automated IR/x86-64 test proving `fn(e: Event) -> Int { e.at }` emits **zero tag loads and zero branches** (one load at constant offset `8`).
6. **Correctness & soundness tests across all tiers (`O0`, `O1`, `O2`, comptime `#run`, Web):**
   - Unit-only enum with shared fields (proves box layout, no `Word` corruption, no singleton aliasing).
   - COW isolation test (`^a: Event.Click{...}; b: a.copy(); a.at = ...; assert_eq(b.at, ...)`).
   - Heap/drop leak-free test with owned `String`/`[Int]` shared fields and variant fields across drop, reassignment, and COW clone.
   - Generic enum with shared fields (type inference from shared field, reads, writes, patterns, reflection).
   - Defaults, `priv` visibility, `#Redact`, computed shared fields, recursive/outsized shared fields.
   - Derived `Equatable` (differing only in shared field -> `false`!), `Comparable`, `Codable` round-trip (external, `#Tag`, `#Untagged`), `Debug`/`Print`, compile-time & runtime reflection (`T.reflect().fields`, `T.reflect().layout`).
7. **UI diagnostic snapshots (I4):** Shared-vs-variant field name collision, shared field after variant, shared field on `#Layout(U8)` (`E1120`), partial `.move()` out of non-copy shared field, missing required shared field on construction, Q2/Q3 diagnostics.
8. **Executable golden example (I5):** Add a dedicated example under `Examples/` with real `.out` golden output exercising shared-field reads, COW writes, pattern matching, derives, and reflection across every tier (`O0`, `O1`, `O2`).
9. **Specs, registry, and grammars:**
   - Update `Compiler/JetFoundation/Data/Syntax/Spellings.jet:19`.
   - Update `Docs/Spec/spec.md`, `Docs/Spec/Reference/metaprogramming.md:559`, and `Docs/Spec/Reference/syntax-surface.jet`.
   - Verify `Tools/Editors/TreeSitter/grammar.js` parses the new example cleanly (`Tests/Guards/grammar.jet` with zero ERROR/MISSING nodes).
   - Pass `Tests/Guards --filter diagnostics_coverage`, `american_spelling`, `copy_ratchet`, and `CoreDeclarations`.

---

**Audit verdict:** **NEEDS BALLOT** (resolve Q1–Q4 on Tower; once ratified, proceed directly against the binding cutover list above).
