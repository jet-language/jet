# Adversarial Design Review — `#EveryField(of: T)` Check Marker (`P20` / `#7018`)

- **Date:** 2026-11-26
- **Scope:** Greenfield Proposal Part 2 (`P20`, *Every-field checks*) — `#EveryField(of: T)` refusing any function or method whose body never reads one of `T`'s stored fields, plus `#EveryField(of: T, except: [.cache])` once `#7008` (`KeyPath`) lands.
- **Baseline inspected:** `dev` (`b84063a8e0f`)
- **Related audits:** `Docs/Audits/2026-11-26-typed-key-paths-review.md` (`#7008`)
- **Verdict:** **APPROVE TO BUILD (`c2` + base `c4`); `c3` GATED ON `#7008`** — `#EveryField(of: T)` is a clean, additive, 100% bootstrap-safe compile-time `MarkerHook` check with zero parser changes and zero legacy syntax to retire. All core `c2` rules follow unambiguously from P20 and existing Jet semantics; `c3` (`except: [.cache]`) MUST wait for `#7008`'s key-path literal spelling rather than inventing a throwaway string/ident skip list that would create a dual surface.

---

## 1. Proposal Summary & Criteria Split

Greenfield Proposal Part 2, item `P20` (`Docs/Proposals/2026-11-24-greenfield-language.md:510-516`) specifies:

- **Language:** `#EveryField(of: T)` on a function (or method) fails to compile when the function body never reads one of `T`'s fields.
- **Motivation:** Derives cover generated codecs, equality, ordering, and display; no compile-time guard exists for hand-written `save`, `compare`, `copy`, or `migrate` functions when a new field is later added to `T`.

Card `#7018` splits cleanly into two delivery stages:
- **Stage 1 (`c2` + base `c4` — UNBLOCKED NOW):** Built-in `#EveryField(of: T)` check marker (`sites: [.Function, .Method]`, `repeatable: true`), registered diagnostics naming every missed stored field (plus invalid non-struct target and inaccessible private fields), UI error snapshots, golden executable example, spec/reference docs, and Tree-sitter marker highlight update.
- **Stage 2 (`c3` — BLOCKED ON `#7008`):** Add optional `except: [AnyKeyPath<T>]` (using `#7008`'s ratified key-path type and `.field` literal spelling) to record deliberate omissions, rejecting any skipped field that does not exist (or was deleted) on `T`.

---

## 2. Strongest Existing Solutions & Primary Sources

| System & Primary Source | Mechanism | Strengths & Failure Modes / Lesson for Jet |
| --- | --- | --- |
| **Rust exhaustive struct destructuring & `clippy::exhaustive_structs`** (The Rust Reference §8.2.9 *Struct Patterns*; `#[non_exhaustive]` RFC 2008) | Authors write `let Order { id, total, date } = order;` (without `..`) at the top of hand-written `save`/`eq`/`hash` functions; adding a field fails compilation. | **Zero compiler magic, but noisy and brittle:** forces destructuring every field into locals even when the function uses `order.id` directly or takes two `Order` values (`a`, `b`); easy to break accidentally with `..` or `field: _`. Also demonstrates the **private-field visibility rule** (`#[non_exhaustive]` / private fields forbid exhaustive destructuring outside the defining module). |
| **Go `exhaustruct`** (G. Boyko, `github.com/GaijinEntertainment/go-exhaustruct`, golangci-lint) | External AST linter checking that struct composite literals specify every field. | **Checks construction, not reading** (Jet already checks non-defaulted fields on struct literals!). Also suffers from opt-out drift: regex exclude lists in YAML silently rot when fields are renamed or deleted — validating `#7018 c3`'s rule that a stale entry in `except:` MUST be a compile error. |
| **TypeScript mapped types & `satisfies`** (TypeScript 4.9 Release Notes; `Record<keyof T, ...>`) | Encodes field exhaustiveness in the type system via key-indexed handler tables (`{ [K in keyof T]: (v: T[K]) => void }`). | Works for uniform dispatch tables, fails ergonomically for ordinary imperative `save`/`compare`/`migrate` functions with control flow, early returns, and heterogeneous field types. |
| **Zig `@typeInfo(T).@"struct".fields` + `inline for`** (Zig Language Reference §39.5) | Compile-time loop iterates over every field structurally at zero runtime cost. | Ideal for uniform serialization/hashing (which Jet already solves with derives and `T.reflect().fields`), but does not protect *custom* per-field hand-written logic (e.g., column-specific SQL `INSERT`, bespoke schema migrations, custom diffing). |
| **Swift / Kotlin synthesized `Equatable`/`Codable` vs manual methods** (Swift `SE-0185`, Kotlin `data class`) | Compiler synthesizes full-field `==`/`hash`/`copy`, but drops to zero exhaustiveness checking the instant an author writes a custom `==` or `encode(to:)`. | Exact pain point P20 solves: opting out of a derive for one custom rule currently forfeits all field-addition protection. |

**Takeaway:** Rust's exhaustive destructuring is the closest battle-tested analog, and `#EveryField(of: T)` strictly improves on it by keeping normal `order.field` code readable, working naturally with two-argument `compare(a, b)` and method receivers `self`, forbidding silent `..` / `field: _` bypasses, and turning stale skip-list entries into compile errors.

---

## 3. Complete Live-Code Inventory on `dev` (`b84063a8e0f`)

| Area | Exact Files & Line Ranges | Live Symbols & Current Behavior | Relevance to `#EveryField` |
| --- | --- | --- | --- |
| **Prelude Marker Vocabulary** | `Compiler/Prelude/Markers.jet:1-250,765-771`; generated copy `Compiler/JetFoundation/Source/Policy/PreludeMarkers.jet:1-6` | Markers are structs with `impl Marker { sites: [...], repeatable: true }`. `SQL` at `L765-771` already declares a `Type` parameter (`row: Type{none}`). `PreludeMarkers.jet` is generated from `Compiler/Prelude/Markers.jet` by `jet build Compiler` (`Compiler/Toolchain/PreludeSources.jet`). | Add `EveryField` to `Compiler/Prelude/Markers.jet` (never hand-edit `PreludeMarkers.jet` alone without keeping it synced via build). Zero lexer/parser syntax changes needed. |
| **Marker Names & Hooks** | `Compiler/JetFoundation/Source/Policy/MarkerNames.jet:1-85`; `MarkerHooks.jet:22-108`; `Data/Syntax/Markers.jet:1-30` | `MarkerHook` enum keys compiler-enforced Prelude hooks by declaration identity (not raw name, so user-shadowed `#Other.EveryField` never falsely triggers). | Add `MARKER_EVERY_FIELD: "EveryField"` and `MarkerHook.EveryField`. Covered by `Tests/Policy/MarkerNames.jet` and `JetParser/Tests/Parser/MarkerRegistryCoverage.jet`. |
| **Marker Arg Parsing & Rules** | `Compiler/JetFoundation/Source/AST/Statements.jet:238-252` (`Marker`); `Compiler/JetSema/Source/Sema/MarkerRules.jet:145-183` (`sema_marker_rules_type_kind`) | `Marker` already parses labeled `args: [MarkerCallArg]` and `arg_labels: [CallLabel?]`. **Gap:** `sema_marker_rules_type_kind` currently maps `Type` to `TAppliedRuleArgKind.Ident` (no dedicated `Type` resolution kind). | Must explicitly resolve `of:` as a type expression (supporting qualified paths like `pkg.Order` and generic heads/applications `Box` / `Box<Int>`). |
| **User-Space `check` Rules** | `Compiler/JetSema/Source/Sema/Calls/CheckRules.jet:1-100`; `Calls/Resolution.jet:142` | Marker `check: fn(self, call: CompilerCall)` rules ONLY hear **call sites** (`CompilerCall`) and `#Recorded` calls; they have no access to `FnBody.tree` or field reads. | Proves `#EveryField` **must** be a built-in `MarkerHook` in Sema, not a user-space `check` rule. |
| **Whole-Program Unread Field Lint (`L0106`)** | `Compiler/JetSema/Source/Sema/Liveness.jet:477-825,1230-1231` | `sema_liveness_member_body_uses` walks `FnBody.tree`, excluding plain write targets (`Assign(place, op == None)` at `L740`, while counting compound `+=`) and counting `.Field(_, TFieldRef)`, `.OptField`, `StructDestructure`, `BodyPatternShape.Struct`, `SharedGuardMap`/`Split`, and `CellGuardProject`. | **Crucial warning:** `Liveness.jet:514-516` puts `StructDestructure`, `Pattern.Struct`, and `OptField` into an **untyped** `field_names: NameTable` and marks whole-value `print(x)`/`equal`/`generic` calls as reading every field (`L484-490`). Both shortcuts are **unsound** for `#EveryField`! |
| **Method Receiver Footprints (`E0231`)** | `Compiler/JetSema/Source/Sema/Footprint.jet:1-256`; `FunctionCheck.jet:1160-1162` | Computes per-method upper bound (`may-touch` set) of stored fields (`if field.computed == None` at `L180`) across `Field`, `MemberSpread` (`self.[a, b]`), and sibling `self` calls. | Confirms stored-vs-computed field rule (`!field.computed`), but `Footprint.jet` computes a *may-touch upper bound* (writes + whole-`self` fallback), the dual opposite of `#EveryField`'s *must-read lower bound*. |
| **Typed Body & Field Metadata** | `Compiler/JetFoundation/Source/AST/Items.jet:1364-1376` (`Field`); `Facts/Declarations.jet:179-190` (`TField`); `Facts/Checked.jet:264-269` (`TFieldRef`); `Facts/BodyStmt.jet:14` (`StructDestructure`) | `TField` carries `name`, `public`, `package_public`, `computed`, `has_default`, `skip`, `redact`. `TFieldRef` carries `owner: SymbolID?`, `name: String`, `ty: Type`. `BodyTree.expr_types` maps every expression node id to its checked `TypeID`. | Provides exact typed owner resolution for every field read, destructuring init (`temp.ty` / `init`), pattern scrutinee, and optional-field receiver. |
| **Function Checking Pipeline** | `Compiler/JetSema/Source/Sema/FunctionCheck.jet:903-1050` | Checks function markers (`L905`), body statements (`L978`), ownership/liveness (`L1012-1021`), and builds `FnBody` (`tree` + `type_delta`). | Clean single hook point immediately after `FnBody` tree construction (skipping when the body has unresolved-name/type errors, matching `L1012`). |
| **Diagnostics & Editor Grammars** | `Compiler/Prelude/Diagnostics.jet` (free `E09xx`: `E0972`, `E0974`, `E0976`, `E0978`–`E0982`); `Registry/DiagnosticRows.jet`; `Tools/Editors/TreeSitter/grammar.js:24` (`JET_HIGHLIGHT_MARKER_RULE`) | Registered diagnostic table + `JET_HIGHLIGHT_MARKER_RULE` sorted marker list. | Register dedicated `E0972` (missed fields), `E0974` (non-struct target or inaccessible private fields), and reserve `E0976` (`c3` unknown/stale `except:` field). Add `"EveryField"` to `grammar.js:24`. |

---

## 4. Adversarial Attacks & Edge-Case Analysis

### Attack 1 — Why `#EveryField` Cannot Be a User-Space `check` Rule
Jet's user-defined marker `check` rules (`Sema/Calls/CheckRules.jet:1-100`) only execute on `CompilerCall` records at call sites (`#Recorded` calls). They cannot inspect a function's `BodyTree`, resolve `TFieldRef.owner`, distinguish plain assignment targets (`x.f = v`) from reads (`x.f`), or inspect struct destructuring patterns. Exposing the entire typed `BodyTree` to user-space comptime would freeze compiler IR as public API. Therefore `#EveryField` **must** be a built-in Prelude marker with `MarkerHook.EveryField` evaluated inside `JetSema`.

### Attack 2 — Pure Writes (`x.f = v`) vs Reads (`x.f`, `x.f += v`) vs Construction (`T{...}`)
- **Plain assignment (`x.f = expr`):** Overwriting a field does **not** read its previous value (`Liveness.jet:737-742`). Counting plain assignment as a read would let a broken `migrate` or `copy` function overwrite `dst.f = 0` without ever reading `src.f`.
- **Compound assignment (`x.f += 1`, `x.f -= 1`):** Reads the old value before writing (`op != None` on `BodyStmt.Assign`); **counts as a read**.
- **Mutating method call on a field (`x.items.push(v)`, `^x.buf.clear()`):** The field expression `x.items` is evaluated as the receiver of a method call (NOT the LHS of a plain `Assign`); **counts as a read** (the function touched and used that field's state).
- **Struct literal construction (`Order{id: ..., total: ...}`):** Constructs `Order`; does NOT read any field of an input `Order` unless the value expressions themselves read `src.id`, `src.total`, etc. Notably, if a new field `discount: Int{0}` has a default, `Order{id: src.id, total: src.total}` compiles normally — and `#EveryField(of: Order)` catches the missed read of `src.discount`!
- **Non-goal clarified:** A pure initializer/resetter `fn reset(^o: Order) { o.a = 0; o.b = 0 }` *writes* fields rather than reading them. `#EveryField` strictly checks **reads**, matching P20 verbatim; write-exhaustiveness for mutators is a distinct concept (and for constructors is already handled by struct literals).

### Attack 3 — Passing Whole `x: T` to Helpers, `print(x)`, `x.copy()`, or Methods
**This is the single most dangerous trap in the design.**
In `Liveness.jet:484-490`, passing `x: Order` to `print(x)`, `x.equal(y)`, `json.encode(x)`, or any generic call marks *every* field of `Order` as read to prevent false-positive dead-field warnings.
- **If `#EveryField` reused that rule:** A single debug `print(order)` or `cache.put(order.copy())` inside `save(order: Order)` would mark *all* fields read and **silently defeat `#EveryField(of: Order)` completely!**
- **What about calling a helper `save_header(order)` or method `order.subtotal()`?**
  - Making `#EveryField` transitively follow arbitrary helper functions or methods creates hidden cross-function coupling: removing a field read inside `helper()` would break a distant `#EveryField` caller, and passing `order` to `log(order)` (which prints it) would bypass the check.
  - **Strict, sound rule:** `#EveryField(of: T)` is **strictly intra-procedural and direct**. Passing whole `x: T` to a function, `print`, `x.copy()`, `x == y`, or calling a method `x.foo()` on `x` does **not** count as reading any stored field of `T`. To delegate cleanly, pass the explicit fields (`save_header(order.id, order.created_at)`) or call `order.shipping` directly (reading `Order.shipping` and delegating `Address` to a helper with `#EveryField(of: Address)`).
  - *Note:* If a stored field itself holds a function value (`handler: fn() -> Int`), calling `order.handler()` IS a direct read of field `handler` (same distinction `Footprint.jet:46` makes).

### Attack 4 — Owner-Typed Matching (Fixing `Liveness.jet`'s Untyped `field_names` Shortcut)
`Liveness.jet:514-516` records fields from `StructDestructure`, `BodyPatternShape.Struct`, and `OptField` by bare string name without checking the owner type. For `#EveryField(of: Order)`:
```jet
#EveryField(of: Order)
save: fn(order: Order, user: User) {
    User{id, name}: user   // MUST NOT satisfy Order.id or Order.name!
    write(order.total)
}
```
An untyped `field_names` set would falsely mark `Order.id` as read because `User` also has a field named `id`!
**Required rule:** Every counted field read MUST match the target struct's resolved nominal `SymbolID` (and concrete type arguments when `of:` is instantiated — see Attack 8):
- `BodyNodeKind.Field(base, field_ref)`: check `field_ref.owner == Val(target_symbol)` (and when `of:` specifies concrete type args like `Box<Int>`, check `base`'s checked `Type` in `tree.expr_types` matches `Box<Int>`).
- `BodyNodeKind.OptField(base, member, _)`: inspect `base`'s unwrapped `Option<Inner>` type in `tree.expr_types`; count `member` **only** if `Inner` resolves to `target_symbol` (and matching type args).
- `BodyStmt.StructDestructure(temp, init, _, binds, _)`: inspect `temp.ty` (already typed in `TLocal`!); count `bind.field` **only** if `temp.ty` resolves to `target_symbol` (and matching type args), and skip wildcard discards (Attack 5).
- `BodyPatternShape.Struct(fields, _)` (in `if let`, `match`, `RefutableBind`, `PatternMatches`): inspect the pattern's resolved struct type (from scrutinee / pattern type in `BodyTree`); count named non-wildcard fields **only** when matching `target_symbol`.
- `MemberSpread` (`order.[id, total]`): Sema lowers `MemberSpread` into individual typed field reads / tuples in `BodyTree`; verify each lowered field read carries `TFieldRef.owner == Val(target_symbol)` (add explicit UI test!).
- `SharedGuardMap` / `SharedGuardSplit` / `CellGuardProject` (for `Shared<Order>` projections): check each `TFieldRef.owner` / base type against `target_symbol`.
- `#7008` key-path reads (`order[.id]` once `#7008` lands): `KeyPath<Order, V>` read projection counts `.id` as read on `Order`.

### Attack 5 — Struct Destructuring, Rest Patterns (`..`), and Wildcard Discards (`field: _`)
- **`..` rest pattern (`Order{id, ..}: order`):** Omitted fields covered by `..` are **never** counted as read.
- **Explicit wildcard discard (`Order{id, cache: _}: order`):** Binding a field to `_` explicitly throws it away rather than reading it. It MUST **not** count as reading `cache` — deliberate omissions belong in `#EveryField(of: Order, except: [.cache])` (`c3`), where the omission is visible in the function signature and audited if `cache` is later deleted.
- **Unused local binding (`Order{id, cache}: order` where `cache` is never used):** Reading the field in the destructure reads it from `Order`, while Jet's existing unused-local lint (`L0101` / `Checks.jet:1315`) immediately flags the unused `cache` binding, preventing silent dead-bind workarounds.

### Attack 6 — Computed Fields (`TField.computed`) & Private Fields Across Modules
- **Computed fields (`full_name: String -> "{first} {last}"`):** `TField.computed == true` (`Facts/Declarations.jet:187`). Computed fields are derived views, not stored data (matching `Footprint.jet:180` and `SerdeItems.jet`). `#EveryField(of: T)` checks **stored fields only** (`!field.computed`). Reading a computed field `order.full_name` does **not** automatically mark `first`/`last` as read (by Attack 3, computed field bodies are separate projections).
- **Private fields across module boundaries:** If module `B` writes `#EveryField(of: a.Order)` where `a.Order` has a stored field `secret_seq: Int` that is neither `pub` nor `package pub` (or is `package pub` from another package), module `B` **cannot legally read `secret_seq`**.
  - **Wrong answer:** Silently skip private fields (would let module `B` think it exhaustively saved/compared `a.Order` while silently dropping `secret_seq`!).
  - **Correct answer (fail closed):** Refuse `#EveryField(of: a.Order)` at compile time (`E0974`) naming the inaccessible private field(s), explaining that `#EveryField(of: T)` requires visibility to every stored field of `T` (so it must live in `T`'s defining module/package, or `T` must expose those fields).

### Attack 7 — Control Flow, Early Returns, Dead Branches, and Nested Closures
- **Body-wide read presence (not path-sensitive must-read):** Hand-written `compare(a: Order, b: Order) -> Bool` short-circuits on the first mismatch (`if a.id != b.id -> return false; if a.total != b.total -> return false; true`). `a.total` is NOT read on every execution path. Therefore `#EveryField` MUST check whether each stored field is read **at least once anywhere in the function's checked `BodyTree`** (including inside `if`/`match` arms, loops, and nested closures/lambdas defined inside the body).
- **Dead / unreachable branches (`if false { _ = order.id }`):** Standard unreachable-code lints handle trivially dead branches; `#EveryField` does not need a dataflow solver or SMT pass, keeping it fast, predictable, and 100% deterministic across `O0`/`O1`/`O2`/comptime/Web.
- **Error suppression on broken bodies:** Matching `FunctionCheck.jet:1012` (`!sema_has_unresolved_name_since` / `!sema_has_error_since`), skip `#EveryField` when the function body already has parse/type/unresolved-name errors so users are not spammed with spurious "missed field" noise on half-typed code.

### Attack 8 — Generic Structs, Type Aliases, Non-Struct Types, Empty Structs & Nested Structs
- **Generic structs (`Box<T>`):**
  - On a generic function `copy: fn<T>(b: Box<T>) -> Box<T>`: both `#EveryField(of: Box)` and `#EveryField(of: Box<T>)` resolve to `Box`'s nominal `SymbolID` and check all stored fields of `Box`.
  - On a concrete function `save_int_box: fn(b: Box<Int>, meta: Box<String>)`: `#EveryField(of: Box<Int>)` MUST match both `Box`'s `SymbolID` **and** concrete type arguments `[Int]`, so reading `meta.value` on `Box<String>` does NOT falsely satisfy `Box<Int>.value`! When written without type args (`#EveryField(of: Box)`), any instantiation of `Box` matches.
- **Type aliases:** `#EveryField(of: OrderAlias)` where `type OrderAlias = Order` resolves transparently through the alias to `Order` (preserving instantiated type arguments if `type IntBox = Box<Int>`).
- **Non-struct types (`Int`, `Status` enum, `distinct Port`, `Trait`, unknown name):** Refuse at compile time (`E0974`: `#EveryField(of: ...)` requires a struct type — enums already enforce exhaustive matching).
- **Empty structs (0 stored fields):** Trivially satisfied (0 missed fields).
- **Nested structs (shallow by design):** If `Order` has `shipping: Address`, reading `order.shipping` satisfies `Order.shipping`. `#EveryField(of: Order)` does NOT recurse into `Address`'s fields; callers annotate `save_address` with `#EveryField(of: Address)` to preserve modular boundaries.

### Attack 9 — Methods (`self: T`), Repeatable Markers, and Multiple `T` Parameters
- **Methods (`sites: [.Function, .Method]`):** Works identically on inherent struct methods and `impl` methods (`save: fn(self) -> ...`), counting reads through `self.field`, `self.[a, b]`, and `Order{a, b}: self`. Require explicit `of: T` in `c2` (even on methods) so there is one uniform spelling with zero ambiguity on trait impls or multi-type converters.
- **Repeatable (`repeatable: true`):** A function like `merge(a: Order, b: Account)` can carry both `#EveryField(of: Order)` and `#EveryField(of: Account)`; each marker checks its own `of:` struct independently. Two `#EveryField` markers naming the *same* struct on the same function are rejected (or deduplicated — rejecting duplicate `of: T` avoids conflicting `except:` lists in `c3`).
- **Two parameters of the same type `T` (`compare(a: Order, b: Order)`):** Subtle edge case! In `a.id == b.id && a.total == a.total` (copy-paste bug where `b.total` was never read), type-level checking (`of: Order`) sees `Order.total` read via `a.total` and passes. Why type-level checking is still the right semantics for `#EveryField(of: T)`: functions like `patch(base: Order, defaults: Order)` legitimately read some fields only from `base`, and functions that load `order: Order` locally have zero `Order` parameters. Document this explicitly in the spec so users understand `#EveryField(of: T)` checks coverage of `T`'s schema, not per-parameter balance.

### Attack 10 — Clean `c2` / `c3` Seam With `#7008` (`KeyPath`)
Card `#7018 c3` is explicitly specified as:
> An explicit skip list (`#EveryField(of: T, except: [.cache])`, using key paths from proposal item P8) records deliberate omissions, and a skipped field that no longer exists is an error. *(blockedBy: `#7008`)*

- **Do NOT invent a temporary string/ident `except:` syntax in `c2`.** Shipping `except: ["cache"]` or `except: [cache]` in `c2` and breaking it for `except: [.cache]` in `c3` would violate Jet's no-throwaway-syntax discipline.
- **`c2` ships with `of: Type` only** (zero dependency on `#7008`), giving full value immediately for `save`, `compare`, `copy`, and `migrate`.
- **`c3` adds `except: [AnyKeyPath<T>]{[]}`** once `#7008` lands, with four strict checks:
  1. Every entry must be a compile-time key-path literal on `T` naming a direct stored field (`.cache`); dynamic `KeyPath` variables or nested paths (`.shipping.zip`) are rejected (`E0976`).
  2. Naming a field that does not exist (or was deleted/renamed) on `T`, or naming a computed field, is a compile error (`E0976`).
  3. Duplicate entries in `except:` (`.cache, .cache`) are rejected (`E0976`).
  4. Reading a field that is ALSO listed in `except:`? Recommend either allowing it or warning (do not silently ignore stale `except:` entries if the function starts reading the field — a crisp lint or note keeps `except:` truthful).

---

## 5. Normative Specification (`c2` Locked Rules & I9 Parity)

| Rule | Locked Behavior |
| --- | --- |
| **Declaration (`Compiler/Prelude/Markers.jet`)** | `EveryField: struct { of: Type, impl Marker { sites: [.Function, .Method], repeatable: true } }` (`c3` adds `except: [AnyKeyPath]{[]}` using `#7008`'s ratified type). |
| **Hook & Phase** | `MarkerHook.EveryField`, evaluated in `JetSema` (`FunctionCheck.jet`) on the checked `FnBody.tree` immediately after body checking (skipped if the body already emitted errors). |
| **Target `of: T`** | Resolves through type aliases to a struct's `SymbolID` (plus concrete type arguments when instantiated). Non-struct target or inaccessible private stored fields -> **`E0974`**. Duplicate `#EveryField` for the same struct on one function -> **`E0974`**. |
| **Fields Required** | Every **stored** field of `T` (`!field.computed`), in declaration order, minus `except:` (`c3`). |
| **What Counts as a Read** | Owner-matched `.Field` (not LHS of plain `Assign`), compound assignment (`+=` etc.), method receiver (`x.items.push(...)`), owner-matched `.OptField`, owner-matched `StructDestructure` / `BodyPatternShape.Struct` (excluding `..` and `field: _`), `MemberSpread` (`x.[a, b]`), `SharedGuardMap`/`Split`/`CellGuardProject`, and (`c3`) `KeyPath` reads `x[.f]` — anywhere in the function body including nested closures. |
| **What Does NOT Count** | Plain writes (`x.f = v`), struct literal construction (`T{...}`), passing whole `x` to any function/`print`/`equal`/`copy`/generic call, calling methods on `x`, computed-field reads, `..` rest patterns, `field: _` discards, or reads of same-named fields on unrelated structs. |
| **Missing Field Diagnostic (`E0972`)** | Single diagnostic on the function/marker naming **all** missed stored fields in declaration order (e.g., ``function `save` misses field(s) `discount`, `tax_rate` of `Order` ``) with a secondary label/note pointing to the `#EveryField` marker (and struct declaration when in the same module) and an actionable fix hint. |
| **Stale/Invalid Skip (`E0976`, `c3`)** | Compile error when `except:` names a nonexistent, deleted, computed, nested, non-literal, or duplicate field on `T`. |
| **I9 Parity (`O0`/`O1`/`O2`/Comptime/Web)** | `#EveryField` is a pure Sema compile-time check with zero runtime codegen or optimizer interaction, so behavior is 100% identical across `O0`, `O1`, `O2`, comptime `#run`, and Web/Wasm. |

*Optional owner-override notes (neither blocks `c2`):* (1) If the owner ever wants sibling methods on `self` with a solved `receiver_footprint` to count toward `#EveryField`, that can be enabled cleanly as an opt-in later without breaking valid `c2` code; strict intra-procedural checking is strictly safer as the default. (2) If the owner wants a shorthand `#EveryField` (omitting `of:`) on struct methods to default to `Self`, `of: Type{none}` can be relaxed later in a backward-compatible tweak.

---

## 6. Binding Cutover List & Implementation Plan

Because `#EveryField` is purely additive, **zero legacy syntax or runtime copies are deleted** — and zero bootstrap hazards exist. Every touchpoint required for a complete, leak-free landing:

### Stage 1 (`c2` + base `c4` — Ready to Build Immediately)
1. **Prelude & Policy Registries:**
   - `Compiler/Prelude/Markers.jet`: add `EveryField` marker struct (`sites: [.Function, .Method]`, `repeatable: true`, `of: Type`).
   - `Compiler/JetFoundation/Source/Policy/PreludeMarkers.jet`: regenerate/sync `MARKER_SOURCE` from `Compiler/Prelude/Markers.jet`.
   - `Compiler/JetFoundation/Source/Policy/MarkerNames.jet`: add `MARKER_EVERY_FIELD: "EveryField"`.
   - `Compiler/JetFoundation/Source/Policy/MarkerHooks.jet`: add `MarkerHook.EveryField` and its `MARKER_HOOKS` row.
   - `Compiler/JetFoundation/Tests/Policy/MarkerNames.jet` & `Compiler/JetParser/Tests/Parser/MarkerRegistryCoverage.jet`: verify coverage passes with the new Prelude marker.
2. **Diagnostics:**
   - `Compiler/Prelude/Diagnostics.jet` & `Compiler/JetFoundation/Source/Registry/DiagnosticRows.jet`: register **`E0972`** (`Jet.EveryFieldMissed`) and **`E0974`** (`Jet.EveryFieldTargetInvalid` — covers non-struct target, inaccessible private stored fields, and duplicate `#EveryField` on the same struct).
3. **Sema Implementation:**
   - `Compiler/JetSema/Source/Sema/MarkerRules.jet`: handle `of: Type` resolution in marker argument checking (supporting qualified module types, type aliases, and generic heads/applications).
   - New focused module `Compiler/JetSema/Source/Sema/EveryField.jet`: owner-typed `BodyTree` field-read collector (handling `Field` excluding plain-assign LHS, compound assign, `OptField`, `StructDestructure` excluding `_`, `BodyPatternShape.Struct` excluding `..` and `_`, `MemberSpread`, `SharedGuard*`/`CellGuardProject`, generic type-arg matching, stored-only `!field.computed`, cross-module visibility fail-closed check).
   - `Compiler/JetSema/Source/Sema/FunctionCheck.jet`: invoke `sema_check_every_field(...)` on checked `FnBody` when `func.markers` contains `MarkerHook.EveryField` and the body had no prior errors.
4. **Tests & Golden Example:**
   - UI error tests (`Tests/UI/`):
     - `every_field_missing_field.{jet,stderr}` (`E0972` naming multiple missed fields in declaration order)
     - `every_field_write_only_and_whole_pass.{jet,stderr}` (proves `x.f = 1`, `print(x)`, `x.copy()`, and `helper(x)` do NOT satisfy `#EveryField`)
     - `every_field_unrelated_struct_destructure.{jet,stderr}` (proves destructuring `Other{id}` does NOT satisfy `Order.id`, nor does `Order{id, cache: _}` or `..`)
     - `every_field_private_field_cross_module.{jet,stderr}` (`E0974` fail-closed on inaccessible private field)
     - `every_field_non_struct_target.{jet,stderr}` (`E0974` on `enum` / primitive target)
     - `every_field_generic_instantiation.{jet,stderr}` (proves `Box<String>.value` does NOT satisfy `#EveryField(of: Box<Int>)`)
   - Positive coverage (via tests + golden example): direct reads, compound assign, method receiver on field, `MemberSpread` (`o.[a, b]`), `OptField`, destructuring without `_`, `match` struct patterns, computed fields ignored, methods with `self`, repeatable `#EveryField` on two structs, generic functions `#EveryField(of: Box<T>)`, nested closures.
   - Executable golden example: `Examples/Types/EveryField.jet` + `Examples/Types/EveryField.out` (demonstrating hand-written `save`, `compare`, and `copy` guarded by `#EveryField(of: Order)`).
5. **Spec, Reference & Editor Grammars:**
   - `Docs/Spec/spec.md`: document `#EveryField(of: T)` semantics, stored-vs-computed rule, intra-procedural direct-read rule, visibility rule, and planned `except:` skip list.
   - `Tools/Editors/TreeSitter/grammar.js:24`: add `"EveryField"` in alphabetical order to `JET_HIGHLIGHT_MARKER_RULE`.
   - Regenerate any marker/diagnostic reference pages checked by repo guards.

### Stage 2 (`c3` — Immediately After `#7008` Lands)
1. Add optional `except: [AnyKeyPath<T>]{[]}` (using `#7008`'s ratified type and literal syntax) to `EveryField` in `Compiler/Prelude/Markers.jet` + `PreludeMarkers.jet`.
2. Register **`E0976`** (`Jet.EveryFieldInvalidSkip`) in `Diagnostics.jet` + `DiagnosticRows.jet`.
3. Update `Sema/EveryField.jet` to subtract validated direct stored-field key paths from the required field set, reject nonexistent/deleted/computed/nested/duplicate skip entries (`E0976`), and count `x[.field]` key-path reads as reads of `.field`.
4. Add UI tests for valid `except: [.cache]` and `E0976` on stale/nested/computed skips, update `Examples/Types/EveryField.{jet,out}` and `Docs/Spec/spec.md`.
