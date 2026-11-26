# Design Review: Typed Key Paths (Proposal Item P8)

**Date:** 2026-11-26  
**Scope:** Typed key paths (proposed as `Path<T, V>`, written `.field.sub` where expected), `value[path]` read and `^value[path] = v` write indexing, composition (`.then`), unification with `FieldRef<Model, T>`, integration with collections, forms, validation, migrations, and reflection, and I9 parity across `O0`, `O1`, `O2`, comptime, and Web.  
**Verdict:** **NEEDS BALLOT** (4 concrete surface/scope questions require owner resolution before build—most critically the direct prelude name collision with `core.files.Path` used in 437 files, read-only vs. writable path soundness, and the boundary with Part 3.10 predicates).

---

## 1. Problem Statement & Proposal Summary

Proposal item P8 (`Docs/Proposals/2026-11-24-greenfield-language.md`, Part 2 and Part 3.9) introduces **typed key paths**: first-class, compiler-checked field-path values written with a leading dot (`.title`, `.address.city`) where a key path is expected, supporting:
- Reading and writing through `value[path]` / `^value[path] = new_value`
- Path naming (`Display`, `.name()`, `"address.city"`), equality (`e.at == .address.country`), and composition (`path.then(.city)`)
- Unifying field-addressing APIs that today use unchecked strings or a narrow single-field `FieldRef<Model, T>`: forms (`Core/Web/Forms.jet`), sorting/grouping (`sort_by`, `group_by`, `min_by`, `WebTable.sort_by`), validation error locations (`Validate.jet`, `FieldError`), schema migrations (`Migrations.jet`), reactive/store selectors (`Core/Web/Store.jet`), and `#EveryField` (`#7018`).

---

## 2. Strongest Existing Solutions & Contrast with Jet

| Language / Solution | Mechanism & Primary Source | Strengths & Limitations vs. Jet |
|---|---|---|
| **Swift `KeyPath` / `WritableKeyPath` / `PartialKeyPath`** | `\Root.field.sub` syntax; class hierarchy `AnyKeyPath -> PartialKeyPath<Root> -> KeyPath<Root, Value> -> WritableKeyPath<Root, Value> -> ReferenceWritableKeyPath<Root, Value>` (*Swift Evolution SE-0161 "Smart KeyPaths"*, *SE-0249 "Key Path Expressions as Functions"*). | **Gold standard for type safety.** Separates read-only vs. writable paths in the type system so computed properties and optional chains cannot be written through. Supports `root[keyPath: kp]` subscripting, composition (`appending(path:)`), equality/hashing, and `KeyPath<Root, Value>` coercion to `(Root) -> Value`. Cost: runtime heap-allocated descriptor objects on Swift's runtime. |
| **Kotlin `KProperty1` / `KMutableProperty1`** | `Book::title` property references (*Kotlin Language Specification*, "Callable references"; `kotlin.reflect.KProperty1`, `KMutableProperty1`). | Separates read-only (`val`) vs. mutable (`var`) properties and supports `get`/`set` + `.name`. Does not compose nested chains ergonomically (`User::address` cannot be written `User::address.city`) and carries JVM reflection object overhead. |
| **C++ pointers to data members (`V T::*`)** | `&User::address`, applied via `u.*ptr` (*ISO C++23 §7.6.4 "Pointer-to-member operators"*). | True zero-cost compile-time offset representation (single integer offset in ABI) and zero-copy lvalue projection. Cannot compose nested members (`&User::address::city` is invalid), carries no field name metadata for errors/forms/SQL, and cannot step through indirections safely. |
| **Rust `offset_of!` & field-projection RFCs** | `core::mem::offset_of!(User, address.city)` (stabilized in Rust 1.77); field-projection / `Field` trait proposals (*Rust Project Goals*, *RFC 3308*, *keypath/lens crates*). | `offset_of!` computes constant byte offsets safely for nested inline fields, but yields a raw `usize` with no type-checked `V`, no read/write indexing, no name metadata, and no safe handling of boxed/indirect fields. |
| **Haskell Lenses (`lens`, `optics`)** | First-class van Laarhoven lenses `Lens' s a` (*Edward Kmett, `lens`*; *Optics* library) composing with `.` (`address . city`). | Algebraically complete (separates `Getter`, `Lens`, `Prism`, `Traversal`, `AffineTraversal`), supports read/write/modify on nested structs, sums, and maps. High conceptual/type-error barrier, no built-in field name string metadata, and closure-based runtime dispatch unless aggressively inlined. |
| **LINQ / SQLAlchemy / C# `Expression<Func<T, V>>`** | Captures expression trees as data (*C# Language Specification*, "Expression tree types"; *SQLAlchemy Core*). | Solves `.price < 20 && .year > 2000` SQL translation cleanly by lifting whole boolean expressions into typed ASTs—demonstrating why **predicates-as-data** (`Predicate<T>`) are a separate mechanism from field key paths, not operator overloading on key paths. |

**Jet's architectural advantage:** Jet owns both Sema and its unified `JetIR` (shared across native x86-64 `O0`/`O1`/`O2`, comptime, and WebAssembly via `JetWeb`). A key path can carry **static name metadata (`"address.city"`) + an inline byte offset (`offset >= 0`)** for O(1) zero-copy place projection on inline struct chains, falling back cleanly to a compiler-synthesized internal projection thunk only when stepping through a boxed/recursive edge or COW enum box (`#7017`)—giving C++'s zero-copy offset speed, Swift's safety and ergonomics, and 100% I9 tier parity.

---

## 3. Complete Inventory of Existing Mechanisms `#7008` Touches or Replaces

Every location in the live repository that currently names fields, resolves leading-dot selectors, or consumes field addresses:

| # | Mechanism & Path | Live State Today | Cutover / Interaction with `#7008` |
|---|---|---|---|
| 1 | **Ambient filesystem `Path`** (`Core/Files/Path.jet:27`, `Compiler/Prelude/Core/prelude.jet:55`, `Compiler/JetSema/Source/Sema/Calls/Resolution.jet:148`) | `Path: path.Path` is already exported bare in `core.prelude` and referenced across **437 files** (`Core/`, `Compiler/`, `Examples/`, `Tests/`). `Core/Compiler/Lang.jet:143` also declares a local `Path`. | **Direct prelude name collision.** Cannot overload `Path` (0 type args) with `Path<T, V>` (2 type args). Requires Ballot Q1. |
| 2 | **`FieldRef<Model, T>`** (`Core/Prelude/Prelude.jet:203-208`, `Compiler/Prelude/Core.rows:198-199`, `Compiler/JetSema/Source/Sema/Expressions/FieldRefs.jet:1-266`, `Sema/Expressions.jet:8296`, `Sema/Calls/Direct.jet:1563`) | Already implements single-field leading-dot `.name` expected-type checking, `priv` visibility enforcement, bindingless-loop `it` refusal (`E-SUBJECT-CALL`), and edit-distance typo suggestions (`E0302`). Stores only `priv name: String`; refuses chains (`.a.b`), computed fields, and read/write indexing. | **Primary replacement / evolution target.** Unify into the canonical key-path type and delete the narrower single-field `FieldRef` wrapper (I8: one mechanism). |
| 3 | **Leading-dot parser & bare member chains** (`Compiler/JetParser/Source/Parser/Expressions.jet:1098-1124`, `Compiler/JetSema/Source/Sema/Expressions.jet:8095-8312`) | Lower-case `.member` already parses as `Expr.Field{base: Ident(""), member}` and the postfix loop already parses `.a.b`, `.a[0]`, `.a?.b`, and `.a.m(x)`. Upper-case `.Case` parses as `Expr.EnumLit`. Sema resolves bare member chains in 3 ways today: (1) `FieldRef<M, T>`, (2) bindingless-loop `it.member`, (3) unary callable subject shorthand `fn(T) -> U` (`sema_expr_subject_shorthand`). | Extend `sema_expr_is_bare_member_chain` and `FieldRefs.jet` to resolve nested field chains (`.address.city`) and explicit typed literals (`KeyPath<User, String>{.address.city}`), preserving unambiguous precedence with `.Case`, `.new(...)`, `it`, and unary callable shorthand. |
| 4 | **Forms (`Core/Web/Forms.jet:105-130`)** | `WebFormTyped.set`, `.validate`, `.blur`, `.focus` already take `FieldRef<Model, T>`. | Migrate cleanly to the unified key-path type (unlocking nested form fields like `.address.city` automatically). |
| 5 | **Collections sorting / grouping / extrema** (`Core/Collections/Collections.jet:182-188`, `Compiler/JetSema/Source/Sema/Calls/Collections.jet:164, 365`, `Sema/Calls/Data.jet:140-261`, `Core/Web/Table.jet:209`) | `List.sort_by` takes `fn(T) -> K` (or `fn(T, T) -> Ordering`)—so `books.sort_by(.title)` already works as a literal via subject shorthand, **but `p: KeyPath<Book, String>{.title}; books.sort_by(p)` does not!** `WebTable.sort_by` (`Core/Web/Table.jet:209`) takes an unchecked `name: String`. | Accept `KeyPath<T, K>` directly in `List.sort_by`/`sort_by_desc`/`group_by`/`min_by`/`max_by` (and `WebTable.sort_by` where typed) without breaking `fn(T) -> K` or binary comparators. |
| 6 | **Index read & write** (`Compiler/JetSema/Source/Sema/Expressions/Literals.jet:1055`, `Sema/Statements/Assignments.jet:1344, 1393`, `Compiler/JetIR/Source/BuildExpr.jet:4912-5020`) | `value[index]` and `^value[index] = v` currently check lists, fixed arrays, maps, and strings only. | Add key-path indexing `value[path]` (read as zero-copy place borrow) and `^value[path] = v` / `value[path] += x` / `value[path].push(x)` (mutable place projection). |
| 7 | **Inline offset-chain lowering precedent** (`Compiler/JetIR/Source/BuildSharedGuards.jet:79-113`, `Sema/Calls/Memory.jet:470-515`) | `ir_shared_guard_path_offset` already walks `[TFieldRef]` summing inline byte offsets (`member.offset`) and rejecting `member.boxed`. | Reuse the exact offset calculation for inline `KeyPath` fast paths; generalize with internal projection thunks when stepping through `member.boxed` or COW enum boxes (`#7017`). |
| 8 | **Validation & `FieldError`** (`Compiler/JetSema/Source/Sema/Registration/Validate.jet:1-140`, `Sema/FailureLaw.jet:105-167`, `Core/Web/Web.jet`, `Core/DB/DB.jet:4303`, `Sema/Registration/CLIEntry.jet:969`) | `validate { check(cond, at: field, "msg") }` accepts only a bare top-level sibling identifier (`Validate.jet:98`) and emits `FieldError{path: "field", reason: msg}`. `[FieldError]` is the non-generic decode/validation error list used across `Core/Web`, `Core/DB`, `CLIEntry`, and `FailureLaw`. | Allow nested key paths in `validate` (`at: .address.country`) and add typed `FieldError` helpers (`FieldError.at(path, reason)`, `err.is_at(.address.country)`) while keeping `FieldError.path: String` non-generic and wire-compatible. |
| 9 | **Patches, Undo & Runtime Reflection** (`Sema/Registration/Patchable.jet:1-110`, `Core/Web/Store.jet:74-286`, `Core/Reflect/Reflect.jet:1-124`) | `#Patchable` generates typed `T.Patch` structs (`apply`/`diff`/`merge`). `WebStore` uses `selector: String` / `listen_selector(project: fn(T) -> U)`. `Core/Reflect/Reflect.jet` has `reflect.of(v) -> Value` and `inspect(String)` (no `parse_patch` shipped yet). | Key paths enable typed `WebStore` watchers (`watch(path, cb)` / path-prefix change matching) and generic typed undo/history helpers (`history.set(^doc, .title, new_val)`) cleanly in Core. |
| 10 | **Migrations** (`Compiler/JetSema/Source/Sema/Migrations.jet:1-200`, `Sema/Registration/SerdeItems.jet:1270-1381`) | `migration { rename old to new; add field: T = default; ... }` currently parses bare field identifiers checked against `StructDef.fields`. | Keep `migration` field checking aligned with key-path validation; any future `#Migration` marker (separate P1 card) can accept `KeyPath` directly. |
| 11 | **Database queries** (`Core/DB/DB.jet:515, 1673-1760`) | `core.db` currently builds SQL strings (`table_rows_joined`, `table_count_by_sql(column: String)`, `policy(table, expression: String)`) and has no fluent `db.books.where(Predicate<Book>)` API yet. | Do **not** hack operator overloading onto `KeyPath<T, V>` for `.price < 20nzd && .year > 2000`; keep SQL predicate lifting in the Part 3.10 Predicate-as-data design consuming `KeyPath` column operands (Ballot Q4). |
| 12 | **Editors, Formatter & Specs** (`Tools/Editors/TreeSitter/grammar.js`, `Compiler/JetParser/Source/Formatter/Expressions.jet`, `Spellings.jet`, `Docs/Spec/spec.md`) | `TreeSitter/grammar.js` already tokenizes `.` and identifiers; `Spellings.jet` documents `FieldRef` implicitly via forms only, not typed key paths. | Verify TreeSitter and `jet fmt` round-trip `.address.city`, `KeyPath<User, String>{.address.city}`, `u[path]`, and `^u[path] = v`; update `Spellings.jet`, `spec.md`, and Reference docs. |

---

## 4. Adversarial Attacks & Edge-Case Analysis

### Attack 1: Direct Prelude Name Collision with `core.files.Path` (437 Files!)
- **Claim attacked:** Proposal P8 writes `Path<T, V>` and `Path<Book, String>{.title}` as a bare prelude type.
- **Evidence from live code:** `Compiler/Prelude/Core/prelude.jet:55` exports `Path: path.Path` (`Core/Files/Path.jet:27`), `Compiler/JetSema/Source/Sema/Calls/Resolution.jet:148` lists `Path` in `SEMA_CALL_PRELUDE_NAMES`, and **437 files** across `Core/`, `Compiler/`, `Examples/`, and `Tests/` use bare `Path` for filesystem paths. Jet does not support arity-overloading one nominal type name (`Path` with 0 type parameters vs. `Path<T, V>` with 2 type parameters).
- **Verdict:** **Blocks build until resolved on a Tower ballot (Ballot Q1).** Naming the type `KeyPath<T, V>` (retiring `FieldRef<Model, T>`) avoids a 437-file breaking rename of `core.files.Path` and eliminates beginner confusion between filesystem paths and field paths.

### Attack 2: Writing Through Read-Only Paths (`^u[path] = v` on Computed Fields or `.parent?.name`)
- **Claim attacked:** Proposal Part 3.9 promises `u[field] = "Wellington"` on `Path<User, String>` AND states *"A path can step through fields (`.address.city`), computed fields (read-only), list elements (`.items[0]`), map entries (`.scores["ana"]`) and optional fields (`.parent?.name`)"*.
- **Soundness flaw:** If a single `KeyPath<T, V>` type accepts computed fields (`name: T -> expr`, which `Spellings.jet:18` and `E0339` forbid assigning, lending for writing, or taking) or optional chains (`.parent?.name`, which may be absent), then a generic function `text_field: fn<T>(label: String, value: ^T, at: KeyPath<T, String>)` cannot know at compile time whether `value[at] = new_text` is legal! Rejecting it only at runtime with a panic would violate **I1** and `E0339`.
- **Additional flaw with `.items[0]` / `.scores["ana"]`:** A list index or map key step is **fallible** (out-of-bounds or missing key) and holds runtime index/key state (not a pure compile-time field offset, violating the "compiles to field offsets" claim and requiring heap storage + bounds checks + COW buffer uniqueness).
- **Verdict:** **Blocks build until resolved on a Tower ballot (Ballot Q2).** Either (A) separate writable `KeyPath<T, V>` (stored fields only) from read-only `ReadKeyPath<T, V>` (widening `KeyPath -> ReadKeyPath`) like Swift's `WritableKeyPath`/`KeyPath`, or (B) scope `#7008` strictly to writable stored-field chains (`KeyPath<T, V>`) first so every `KeyPath<T, V>` is statically proven readable AND writable with zero runtime failure.

### Attack 3: `.price < 20nzd && .year > 2000` vs. `e.at == .address.country` (Fatal Operator-Overloading Ambiguity)
- **Claim attacked:** Proposal P8 lists BOTH `e.at == .address.country` (line 362) and `db.books.where(.price < 20nzd && .year > 2000)` (line 1757) as key-path features.
- **Proof of conflict:**
  1. In `e.at == .address.country`, `==` compares two key paths and MUST return **`Bool`**.
  2. In `db.books.where(.price == .list_price)` or `#Check(.end > .start)`, `==` / `>` compares two fields of one record and MUST build a **`Predicate<T>`** AST!
  3. Furthermore, `&&`, `||`, and `!` in Jet are built-in short-circuiting control operators—they **cannot** be overloaded on a user or Core struct!
- **Resolution:** `.price < 20nzd && .year > 2000` is **NOT** operator overloading on `KeyPath<T, V>`. It is **expected-type predicate lifting** (Proposal Part 3.10: `Predicate<T>` as data + code), where Sema lifts an expression containing bare `.field` references into a typed predicate AST when `Predicate<T>` is expected. Keeping `==` on `KeyPath<T, V>` as ordinary `Equatable -> Bool` makes `e.at == .address.country` completely unambiguous, and cleanly separates `#7008` (typed key paths) from Part 3.10 (predicates-as-data) (Ballot Q4).

### Attack 4: Runtime Representation, Non-Copy Values (`String`, `[Int]`, `#MoveOnly`), COW Safety, and the "Zero Cost" Claim
- **Claim attacked:** *"A path costs nothing at run time; it compiles to the field offsets"* (Part 3.9, line 1789), while also supporting runtime passing (`text_field`), storage (`History<Doc>`), `.name()`/`Display`, equality, and `.then` composition.
- **Hidden traps attacked:**
  1. **Non-Copy fields (`String`, `[Int]`, `#MoveOnly File`):** If `u[path]` lowered to a naive getter closure `fn(T) -> V`, reading `u[path]` on `String` or `[Int]` would either copy implicitly (violating Jet's explicit `.copy()` law!) or fail completely for `#MoveOnly` types! And `u[path].push(x)` (mutating a `[Int]` field in place through a key path) requires a mutable place address, not a returned copy.
  2. **Boxed recursive edges & COW enum shared fields (`#7017`):** In `BuildSharedGuards.jet:85` and `BuildExpr.jet:4980`, a recursive/outsized struct field (`member.boxed == true`) or a payload enum box (`#7017`) is NOT at a single flat offset from the root—writing through it MUST call `ir_wrapper_unique` or `ir_box_make_unique` on the intermediate box so `u.copy()` is never silently corrupted!
  3. **In-place mutability & ownership (`^u[path] = v`):** Assigning `^u[path] = new_val` must drop the old `V` at that address, store `new_val`, and require `u` to be a mutable place (`^u` / `^self`), refusing writes through an immutable binding `u` (same check as `u.field = v`). Partial moves `u[path].move()` MUST be rejected because `path` is dynamic (ownership analysis cannot statically know which field of `u` was moved out to suppress its drop!).
- **Clean, I9-uniform representation:**
  ```jet
  KeyPath: struct<T, V> {
      priv name: String,      // canonical dotted name, e.g. "address.city"
      priv offset: Int,       // >= 0 for pure inline stored chains; -1 when indirect/boxed
      priv project_read: ..., // compiler-internal fn(Ptr<T>) -> Ptr<V> (used only if offset < 0)
      priv project_write: ...,// compiler-internal fn(Ptr<T>) -> Ptr<V> with COW-unique (if offset < 0)
  }
  ```
  - **Fast path (`offset >= 0`, covering ~99% of structs):**
    - `u[path]` read = `addr(u) + path.offset` -> zero-copy place borrow (`IROp.Offset` + `IROp.Load` for scalars, direct address for aggregates; works for `String`, `[Int]`, AND `#MoveOnly`!).
    - `^u[path] = v` write = `addr(u) + path.offset`, drop old `V`, store `v` (zero-copy, in-place!).
    - `u[path].push(x)` = mutable place at `addr(u) + path.offset`!
    - `p1.then(p2)` when both `offset >= 0` = `{ name: "{p1.name}.{p2.name}", offset: p1.offset + p2.offset }` — O(1) integer addition, zero indirect call!
    - When `path` is statically known (e.g. `u[.address.city]` or after O1/O2 inlining of `text_field`), constant folding collapses `addr(u) + const_offset` to the **exact same single load/store instruction as `u.address.city`**!
  - **Indirect path (`offset < 0`, boxed recursive edges / `#7017` enum shared fields / composed indirect paths):**
    - Calls compiler-synthesized `project_read` (no COW) or `project_write` (calls `ir_wrapper_unique` / `ir_box_make_unique` at each boxed hop) — standard `JetIR` functions, giving **100% I9 parity across `O0`, `O1`, `O2`, comptime, and WebAssembly** with zero new backend/WASM instructions!
  - **Comptime & Static Blobs:** Static `KeyPath<T, V>` values fold cleanly in `Comptime/Fold.jet` and `BuildStatic.jet`, so `const` key paths and `#run` inspection (`TITLE.name()`) work identically to runtime.

### Attack 5: Precedence Among 4 Leading-Dot Meanings & Variable Orthogonality (`sort_by(p)`)
- **Competing leading-dot meanings in live Jet (`Sema/Expressions.jet:8095-8312`):**
  1. `.Variant` (upper-case) -> `Expr.EnumLit` (`Parser/Expressions.jet:1124`).
  2. `.new(...)` -> inferred constructor (`Expressions.jet:8111`).
  3. Bindingless loop `it` (`implicit_loop_subject_depth > 0`) -> `it.member` (`Expressions.jet:8177`; `FieldRefs.jet:113` already refuses `FieldRef` inside bindingless loops with `E-SUBJECT-CALL`).
  4. Expected-type disambiguation (outside bindingless loops):
     - Expected `KeyPath<T, V>` (or `AnyKeyPath<T>`) -> typed key path!
     - Expected `fn(T) -> U` -> unary subject shorthand (`fn(x) { x.member }`, `Expressions.jet:8145`)!
     - Expected unconstrained generic `K` or `None` -> compile error directing `KeyPath<Book>{.title}` (greenfield review C17).
- **Orthogonality trap caught:** Today `books.sort_by(.title)` works via subject shorthand (`fn(T) -> K`), **but** extracting `p: KeyPath<Book, String>{.title}; books.sort_by(p)` fails unless `List.sort_by` / `sort_by_desc` / `group_by` / `min_by` / `max_by` also accept `KeyPath<T, K>`! (Ballot Q3.)

### Attack 6: Privacy, `#Secret` Taint, `#Layout(columnar)`, and Generic/Enum Roots
- **Privacy (`priv` fields):** Constructing `.secret_field` checks field visibility at the construction site (`FieldRefs.jet:173-176`). Passing a constructed `KeyPath<Account, String>` out of the module intentionally delegates capability to that one field—never allowing outsiders to forge a path to a `priv` field (and `DynPath` from strings must NEVER bypass `priv`!).
- **`#Secret` taint preservation:** If `User.token` has type `#Secret String`, then `.token` MUST resolve to `KeyPath<User, #Secret String>` (preserving the `Type.Tagged` wrapper!) so `u[path]` stays tainted and cannot bypass `#Secret` sink checks!
- **Generic structs & type inference:** Given `Box<T>: struct { item: T }` and `fn<M, V>(m: M, p: KeyPath<M, V>) -> V`, passing `(Box{item: 42}, .item)` MUST settle `M = Box<Int>` first from arg 1 and then resolve `.item` against `Box<Int>` to settle `V = Int` (exact same multi-pass call inference already pioneered for `FieldRef<M, T>` in `Sema/Calls/Direct.jet:1563`!).
- **Enum roots:**
  - Plain enum without shared fields -> refused (`E0302`, no shared fields).
  - Enum with shared fields (`#7017`) -> `.at` resolves to `KeyPath<Event, Time>` (using `offset = -1` indirect COW-safe projection thunk since `Event` is a counted heap box!).
  - Variant-specific fields (`.Click.x`) -> refused (not present on every value of `Event`).
- **`#Layout(columnar)` (`Rows<T>`):** A `#Layout(columnar)` struct element is materialized/projected by column; a `KeyPath<Row, V>` on a plain `Row` value works normally (inline offset on `Row`), and on `Rows<T>` column projection can inspect `path.name()` cleanly.

### Attack 7: Heterogeneous Collections (`AnyKeyPath<T>`) & `FieldError` Compatibility
- **Gap in Proposal Part 3.9:** `e.at == .address.country` (line 1696) and `History<Doc>` storing changes to both `.title: String` and `.pages: Int` require a **value-type-erased** path `AnyKeyPath<T>` (carrying `name: String`, `segments: [String]`, `value_type: TypeId`) to which any `KeyPath<T, V>` widens implicitly or via `.erased()`!
- **Breaking-change trap on `FieldError`:** `FieldError{path: String, reason: String}` is a non-generic prelude error type wired throughout `Decode`, `Core/Web`, `Core/DB`, `CLIEntry`, and `FailureLaw.jet`. Making `FieldError` generic (`FieldError<T>`) would break every `[FieldError]` boundary. Keeping `FieldError.path: String` and adding typed helpers (`FieldError.at(path, reason)` and `err.is_at(.address.country)`) gives full compile-time field checking without breaking `[FieldError]`.

---

## 5. Open Surface & Scope Questions Requiring Owner Ballot (`NEEDS BALLOT`)

Before any build commit starts on `#7008`, the following **four** choices need owner ratification (bundleable into one ballot):

### Q1. Prelude Type Name (`Path` collision with `core.files.Path` in 437 files) & `FieldRef` Unification
- **Options:**
  - **A (Recommended):** Name the prelude type **`KeyPath<T, V>`** (plus `AnyKeyPath<T>` for value-type-erased paths), **retire `FieldRef<Model, T>`** into `KeyPath<T, V>` across `Core/Prelude`, `Core/Web/Forms.jet`, and `Sema/Expressions/FieldRefs.jet`, and leave filesystem `Path` (`core.files.Path`) untouched across all 437 files.
  - **B:** Evolve existing **`FieldRef<T, V>`** in place into the full key-path type (zero new prelude name, zero `Path` breakage).
  - **C:** Rename filesystem `Path` -> `FilePath` across all 437+ files, `Spellings.jet`, and `Docs/Spec` so bare `Path<T, V>` can claim the prelude name `Path`.

### Q2. Read-Only vs. Writable Paths (Computed Fields, Optional Chains `.parent?.name`, Dynamic Index Steps `.items[0]`)
- **Options:**
  - **A (Recommended — strict stored-field `KeyPath<T, V>` + optional `ReadKeyPath<T, V>` split):** Every `KeyPath<T, V>` is statically proven **readable AND writable** (stored struct/record fields, boxed recursive fields, and `#7017` enum shared fields). Computed fields (`name: T -> expr`) and optional chains (`.parent?.name` -> `V?`) are **read-only** `ReadKeyPath<T, V>` (with automatic widening `KeyPath<T, V> -> ReadKeyPath<T, V>`, rejecting `^u[read_path] = v` at compile time under `E0339`). Dynamic index steps (`.items[0]`, `.scores["ana"]`) stay out of static `KeyPath` (belonging to fallible `DynPath` / explicit indexing).
  - **B (Minimal v1 — stored fields only):** `KeyPath<T, V>` covers writable stored-field chains only (`.address.city`) with ONE prelude type; computed fields, optional chains, and dynamic index steps are refused at compile time until a follow-on card adds `ReadKeyPath`.

### Q3. Explicit Key-Path Literal Spelling & Collection Variable Interoperability
- **Options:**
  - **A (Recommended):** Accept both `KeyPath<Book, String>{.title}` and inferred-value `KeyPath<Book>{.title}` when no expected type is in scope; accept stored `KeyPath<T, K>` variables directly in `List.sort_by` / `sort_by_desc` / `group_by` / `min_by` / `max_by` alongside `fn(T) -> K` and binary comparators.
  - **B:** Require full `KeyPath<Book, String>{.title}` only and require `books.sort_by(fn(b) { b[p] })` for stored path variables.

### Q4. Scope Boundary with Part 3.10 Predicates (`.price < 20nzd && .year > 2000`) and `FieldError`
- **Options:**
  - **A (Recommended):** Scope `#7008` to typed key paths (`KeyPath<T, V>`, `AnyKeyPath<T>`, `u[path]`, `^u[path] = v`, `.then`, `FieldRef`/`WebForm` cutover, `sort_by`/`group_by`/`min_by`, `WebTable`, `Validate` `at: .a.b`, `FieldError.at`/`is_at` keeping `FieldError.path: String`, and `#EveryField` unblock). Route `.price < 20nzd && .year > 2000` SQL/in-memory predicate lifting to the dedicated Part 3.10 `Predicate<T>` card consuming `KeyPath` (preserving `e.at == .address.country -> Bool`).
  - **B:** Expand `#7008` to also design and ship `Predicate<T>`, fluent `core.db` `Table.where(Predicate<T>)`, and a generic `FieldError<T>` rewrite in one card.

---

## 6. Binding Cutover List

Once the ballot resolves Q1–Q4, `#7008` cannot close until **every** item below is delivered and proven:

### A. Core Prelude & Library Unification
1. **`Core/Prelude/Prelude.jet` & `Compiler/Prelude/Core.rows`**:
   - Declare canonical `KeyPath<T, V>` (and `AnyKeyPath<T>`, plus `ReadKeyPath<T, V>` if Q2=A) with `name() -> String`, `segments() -> [String]`, `then<U>(self, next: KeyPath<V, U>) -> KeyPath<T, U>`, `Display`, and `Equatable`.
   - Retire `FieldRef<Model, T>` cleanly (zero duplicate field-reference types, I8).
   - Add `FieldError.at(path, reason)` and `FieldError.is_at(path)` interop helpers while preserving non-generic `FieldError.path: String`.
2. **Core consumers**:
   - `Core/Web/Forms.jet`: migrate `WebFormTyped.set`, `.validate`, `.blur`, `.focus` from `FieldRef` to `KeyPath` (supporting nested `.address.city`).
   - `Core/Web/Table.jet`: add typed `KeyPath` overload/replacement for `WebTable.sort_by` where applicable.
   - `Core/Collections/Collections.jet` & `Compiler/JetSema/Source/Sema/Calls/Collections.jet`: accept `KeyPath<T, K>` in `sort_by`, `sort_by_desc`, `group_by`, `min_by`, `max_by` alongside `fn(T) -> K`.

### B. Compiler (Parser, Sema, IR, Comptime, Formatter)
3. **Parser & Formatter**:
   - `Compiler/JetParser/Source/Parser/Expressions.jet`: verify `.address.city` and explicit `KeyPath<Book, String>{.title}` / `KeyPath<Book>{.title}` parse cleanly without conflicts with `.Case` or `.new(...)`.
   - `Compiler/JetParser/Source/Formatter/Expressions.jet`: ensure round-trip formatting of `.address.city`, `KeyPath<Book>{.title}`, `u[path]`, and `^u[path] = v`.
4. **Sema (`Expressions/FieldRefs.jet` -> `Expressions/KeyPaths.jet`, `Expressions.jet`, `Calls/Direct.jet`, `Expressions/Literals.jet`, `Statements/Assignments.jet`, `Registration/Validate.jet`, `Ownership/`, `Comptime/Fold.jet`)**:
   - Replace single-field `FieldRefs.jet` with nested chain resolution (`.a.b.c`) enforcing visibility at every step, preserving `#Secret` taint tags in `V`, checking generic struct substitution, supporting `#7017` enum shared fields, rejecting variant-specific enum fields, rejecting partial `.move()` out of `u[path]`, and enforcing read-only vs. writable rules per Q2.
   - Update `sema_expression_index` (`Literals.jet:1055`) and `LValue.Index` (`Assignments.jet`) for `u[path]` read (zero-copy place borrow) and `^u[path] = v` / compound assign / in-place mutating method calls (`u[path].push(x)`).
   - Update `Validate.jet` to accept nested key paths (`check(cond, at: .address.country, "msg")`).
   - Support comptime folding of static `KeyPath` values and `.name()` in `Comptime/Fold.jet`.
   - Register any new diagnostics in `Compiler/Prelude/Diagnostics.jet` (I4).
5. **IR & Backends (`BuildTypes.jet`, `BuildExpr.jet`, `BuildStatic.jet`, `JetWeb`)**:
   - Lower inline stored-field chains to `{ name, offset >= 0 }` with direct `IROp.Offset` place reads/writes (zero copy for `String`/`[Int]`/`#MoveOnly`, single offset addition for `.then`).
   - Lower boxed/recursive edges and `#7017` enum shared fields via compiler-synthesized `project_read` / `project_write` (calling `ir_wrapper_unique` / `ir_box_make_unique` on write for COW safety).
   - Lower static/constant `KeyPath` values in `BuildStatic.jet`.
   - Verify automatic I9 parity across `O0`, `O1`, `O2`, comptime, and Web (`JetWeb`).

### C. Verification, Proofs, Docs & Tooling
6. **Zero-cost codegen proof:** Automated IR/x86-64 test proving statically known `u[.address.city]` and inlined `u[path]` compile to a direct constant-offset load/store with zero runtime string lookup or heap allocation.
7. **Multi-tier tests (`O0`, `O1`, `O2`, comptime `#run`, Web):**
   - Nested struct read/write (`u[path]`, `^u[path] = v`, `u[path].push(x)`), non-Copy `String`/`[Int]`/`#MoveOnly` zero-copy read, `.then` composition, `Display`/`name()`/`segments()`/`Equatable`, `AnyKeyPath<T>` heterogeneous list + equality.
   - COW isolation when writing through a boxed recursive edge (`^a[path] = v` never mutates `a.copy()`).
   - Generic structs (`Box<User>`), `WebForm` with nested paths, `List.sort_by(p)` with stored path variable, `validate { check(..., at: .address.country, ...) }`.
8. **UI diagnostic snapshots (I4):** Unknown field with typo edit suggestion (`E0302`), private field access from outside module, ambiguous unconstrained `.title` without expected type, bindingless loop `it` collision (`E-SUBJECT-CALL`), writing to immutable `u[path] = v`, partial move `u[path].move()`, and Q2 read-only write refusal (`E0339`).
9. **Executable golden example (I5):** Dedicated example under `Examples/` with real `.out` golden output exercising nested read/write, `.then`, sorting, forms/validation error paths, and `Display`/equality across `O0`, `O1`, and `O2`.
10. **Specs, registry, and grammars:**
    - Update `Compiler/JetFoundation/Data/Syntax/Spellings.jet`, `Docs/Spec/spec.md`, and `Docs/Spec/Reference/` pages.
    - Verify `Tools/Editors/TreeSitter/grammar.js` (`Tests/Guards/grammar.jet`), `Tests/Guards/CoreDeclarations`, `diagnostics_coverage`, `american_spelling`, and `copy_ratchet` all pass.

---

**Audit verdict:** **NEEDS BALLOT** (resolve Q1–Q4 on Tower before starting any `#7008` build commit).
