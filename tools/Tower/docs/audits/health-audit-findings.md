# Health Audit Findings

> Status: COMPLETE — 2026-06-21.

## Executive Summary

**11 findings across 4 domains.** The repo is in reasonable shape. No ICE-on-malformed-input
panics found in the parser/sema pipeline — the `unwrap`/`expect` calls are all locally
guarded. The most urgent issue is SAFE-1: `owner_type` is stored in `Checker` but never
read back from the struct, so body-checking code cannot know what type a method belongs
to (latent logic bug, not crash). SAFE-2 is the only true I4 invariant breach: 80 diagnostic
codes lack `tests/ui` snapshots, of which ~20 are in the sema core (E04xx uninit, E0019,
E0039, etc.). MAINT-1/MAINT-2 classify all build warnings; most are KEEP or REMOVE with
the LSP ones the only subtle case.

---

## 1. SAFETY & CORRECTNESS

**SAFE-1** · `Source/Sema/mod.rs:444`, `Source/Sema/Registration.rs:1024,1047`
· **`owner_type` field stored but never read from `Checker` struct**
· `Checker` has an `owner_type: Option<String>` field set at construction time
(Registration.rs:1024 / Bundle.rs:1328) but the field is never accessed via `self.owner_type`
anywhere. The useful logic lives in the *local parameter* `owner_type` inside
`check_params_and_body`, consumed only when registering `self` in scope. Once
`check_block(&mut f.body)` runs at Registration.rs:57, any code in the body that needs
to know the enclosing type has no way to get it — the information is silently dropped.
This means features like nested closures in methods that need to resolve `self` type, or
any future code that reaches for `self.owner_type`, will silently get `None`.
· **Risk:** latent logic bug; type-resolution errors in method bodies may be misattributed
or silently missed. Not a crash today, but a correctness hole that will manifest as
wrong-type inference or missed diagnostics as the type system grows.
· **Fix:** either use the field (`self.owner_type` where needed in body-check helpers) or
remove it and keep only the local parameter. If body-check code genuinely never needs it,
delete the field and the two struct-init assignments to eliminate the warning and confusion.
· **Confidence:** High

---

**SAFE-2** · `tests/ui/` vs `Source/` (all diagnostic-emitting files)
· **80 diagnostic codes lack `tests/ui` snapshot — violates I4**
· 249 distinct error codes are emitted in `Source/`; 169 have corresponding `.stderr`
fixtures in `tests/ui/`; 80 do not. Unsnapshotted codes include sema-core paths reachable
from ordinary user input: E0019 (duplicate module import), E0039 (type mismatch in call),
E0048 (nesting limit), E0061 (C-FFI), E0277 (missing trait), E0420–E0425 (D-UNINIT1,
just ratified 2026-06-21), E0602–E0611 (file I/O errors), E0801–E0804 (comptime),
E0966–E0983 (system/deploy), E1201–E1216 (jetpack), E2001–E2002, E2201–E2202,
E2401/E2406, E2601–E2604, E2901, E3201, E3205, E3302, E3402, E1801–E1802.
· **Risk:** I4 breach — per CLAUDE.md "No snapshot → the diagnostic doesn't exist." Any
rendering regression, code-number change, or voice-rule violation in these 80 paths is
invisible to CI. E0420–E0425 are brand-new (D-UNINIT1) and have no test coverage at all.
· **Fix:** For the 6 D-UNINIT1 codes (E0420–E0425) add snapshots immediately as part of
the ratification work. For the others, triage by call-site reachability and add coverage
in order of user-impact. E0019/E0039/E0048 should be first.
· **Confidence:** High

---

**SAFE-3** · `Source/Codegen/Expression.rs:798`
· **Codegen emits `.remove(idx).unwrap()` into user Rust — silent panic at runtime**
· The `"remove"` method on a non-Map collection emits:
`(list).remove(idx as usize).unwrap()`. `Vec::remove` panics if `idx >= len`.
The sema layer has no bounds check for list `remove` calls, so an out-of-bounds index
becomes a Rust `panic!` in the compiled binary at runtime — visible to end users as a
crash with no Jet error message.
· **Risk:** I3 border case — codegen delegates an unchecked panic to the runtime. Not an
ICE (it's not a compiler crash), but it violates the "batteries included, no footguns for
beginners" promise and produces an opaque Rust-style panic.
· **Fix:** Either add a sema lint when `remove` is called with a non-literal index, or
emit a checked wrapper in codegen that produces a Jet-style bounds-error diagnostic at
runtime. The runtime path is more complete; the sema lint is simpler.
· **Confidence:** High

---

## 2. PERFORMANCE

**PERF-1** · `Source/Sema/CheckerCore.rs:1412–1415`
· **`self.moved` HashSet cloned inside per-arm loop in `check_switch`**
· Each switch arm resets `self.moved = move_before.clone()`. For a switch with N arms
and M tracked moved names, this is O(N×M) cloning in the sema hot path. For exhaustive
enums with many variants (common in domain-logic code), M can be large if many local
bindings exist.
· **Risk:** Plausible real cost on programs with large enums or deeply nested matches.
Similar clone-per-branch patterns exist in `check_if` (CheckerCore.rs:464–472) and
`check_for` (CheckerCore.rs:1176–1208). Not a correctness issue; a compile-time slowdown.
· **Fix:** Replace `HashMap/HashSet` with a persistent/copy-on-write structure (e.g.
`im::HashSet` — but that requires a crate, violating I6). Alternatively, restructure to
store a diff list and replay forward rather than restoring from a clone, or use index-based
move-tracking to make clone O(1). The cleanest std-only fix is to represent `moved` as a
`Vec<(String, Span)>` and record a "watermark" index to restore on each arm.
· **Confidence:** Med (depends on realistic program size; microbenchmark not run)

---

**PERF-2** · `Source/Sema/CheckerInfer.rs:94–496` (`infer_inner`) and `:1920–2330` (`infer_method_call`)
· **400-line match arms — no inlining budget, hard to optimize or test in isolation**
· `infer_inner` spans ~400 lines as a single function with a monolithic `match` on `Expr`.
`infer_method_call` is similarly 410 lines. Rust's monomorphization and the sheer size
mean the compiler may not inline hot sub-paths effectively, and the linker sees one large
function for PGO/profile-guided optimization.
· **Risk:** Secondary; the impact is compile time of the Jet compiler itself, not user
programs. Reportable because it affects iteration speed and readability.
· **Fix:** Split into one function per `Expr` variant arm (similar to how Parser delegates:
`infer_lambda`, `infer_call`, etc.). Each is then independently testable.
· **Confidence:** Low (style/maintainability as much as perf)

---

## 3. MAINTAINABILITY

**MAINT-1** · Build-warning classification — dead code
· The following warned items are **REMOVE** (genuinely dead, safe to delete):

| Symbol | File:line | Verdict | Reasoning |
|--------|-----------|---------|-----------|
| `fn type_known` | `Sema/CheckerCore.rs:256` | **REMOVE** | Only called by itself recursively; no external caller. Replaced by `core_type_known` in `CheckerCoreLib.rs`. |
| `fn check_branches` | `Sema/CheckerCore.rs:463` | **REMOVE** | No callers anywhere. Well-written function that was likely prepared for a refactor that landed differently. `check_if` / `check_switch` implement the same idea inline. |
| `fn enum_lit_args` | `Parser/Expressions.rs:1269` | **REMOVE** | No callers. The enum literal arg parsing is done inline at the call sites. |
| `fn impl_def` | `Parser/Items.rs:1209` | **REMOVE** | No callers. Probably left over from an earlier parser structure. |
| `fn module_path` | `Parser/Items.rs:237` | **REMOVE** | No callers. Import path parsing uses `import_decl_module_path` instead. |
| `tree_hash: String` field in `ResolvedDep` | `Fetch.rs:115` | **REMOVE** | Set at construction (lines 287, 367, 383) but never read back. Dead storage. |

The following warned items are **KEEP + `#[allow(dead_code)]`** (planned/wired-elsewhere):

| Symbol | File:line | Verdict | Reasoning |
|--------|-----------|---------|-----------|
| `fn find_def` | `LSP/SymbolDB.rs:92` | **KEEP** | LSP go-to-definition infrastructure for roadmap M13. Called only by `name_at_offset`. Add `#[allow(dead_code)]` until M13 lands. |
| `fn def_at_offset` | `LSP/SymbolDB.rs:97` | **KEEP** | Called by `name_at_offset` (line 106) — not actually dead, just private. Warning is a false positive from visibility. |
| `fn name_at_offset` | `LSP/SymbolDB.rs:104` | **KEEP** | M13 LSP API surface. Add `#[allow(dead_code)]`. |
| `fn all_refs` | `LSP/SymbolDB.rs:117` | **KEEP** | M13 LSP rename/reference infrastructure. Add `#[allow(dead_code)]`. |
| `SemanticToken::PROPERTY/PARAMETER/NAMESPACE/FUNCTION/ENUM_MEMBER/DECLARATION` | `LSP/Features.rs:245–259` | **KEEP** | These are the semantic-token type constants for LSP c41 (highlighting). They map to the LSP spec integers and will be used when semantic-token responses are wired. Add `#[allow(dead_code)]` on the `SemanticToken` impl block. |

The following are **INVESTIGATE-BUG** (see SAFE-1 above):

| Symbol | File:line | Verdict | Reasoning |
|--------|-----------|---------|-----------|
| `owner_type: Option<String>` field | `Sema/mod.rs:444` | **INVESTIGATE-BUG** | See SAFE-1. Stored but never read; body-check code cannot access it. |

The following are **KEEP, no annotation needed** (already handled or correctly threaded):

| Symbol | File:line | Verdict | Reasoning |
|--------|-----------|---------|-----------|
| `allow_struct_lit: bool` param | `Parser/Expressions.rs` (many) | **KEEP** | Used at lines 622, 1075, 1091 in `expr_postfix` / `expr_primary`. Warning fires on intermediate pass-through functions that don't touch it directly. `#[allow]` on those intermediate fns is acceptable, or restructure to thread a parsing context struct. |
| `color: bool` param | `Source/Diagnostics.rs:224,238,242` | **KEEP** | Used at lines 243, 283, 284, 287. No warning; included here for completeness. |

· **Confidence:** High (all verified by grep)

---

**MAINT-2** · `Source/Sema/CheckerCore.rs:463` (and `CheckerInfer.rs`, `CheckerItems.rs`)
· **Duplication: `check_if`, `check_switch`, and `check_branches` all clone `self.moved`/`self.uninit` with identical restore logic**
· Three separate branch-checking sites each roll their own save/restore idiom for `moved`
and `uninit`. `check_branches` was clearly written to unify them but never got adopted.
· **Risk:** Any future change to the branching invariant (e.g. adding a new tracking set
like `uninit`) must be applied in 3 places.
· **Fix:** After removing the dead `check_branches` (MAINT-1), introduce a well-named
`check_branching_block` helper that takes a slice of branch bodies, and call it from
`check_if` and `check_switch`. Keep the exhaustiveness/coverage logic in `check_switch`.
· **Confidence:** High

---

## 4. WRITING / CLARITY

**CLARITY-1** · `Source/Parser/Statements.rs:652,667,962,1001` and `Source/Parser/mod.rs:320–323`
· **Diagnostic messages use banned words "statement" and "expression" — violates diagnostics.md voice rules**
· 9 diagnostic `what`/`why`/`fix` strings use "statement" or "expression in expression
position" — terms explicitly banned by `docs/spec/diagnostics.md` ("Banned: *token,
expression, statement, identifier, parse, syntax error, illegal, invalid, lifetime, borrow
checker*").

Specific violations:
- `Parser/mod.rs:320`: `"expected the end of this statement, found …"` → say `"something unexpected appeared after this line"`
- `Parser/mod.rs:321`: `"each statement goes on its own line"` → `"each line is its own instruction"`
- `Parser/mod.rs:323`: `"put the next statement on a new line"` → `"move this to the next line"`
- `Parser/Statements.rs:652`: `"a statement must have an effect"` → `"this line must do something"`
- `Parser/Statements.rs:667`: `"expected a statement, found …"` → `"expected a call, binding, assignment, or \`return\`, found …"`
- `Parser/Statements.rs:962`: `"in expression position both outcomes …"` → `"when used as a value, both branches must produce a value"`
- `Parser/Statements.rs:1001`: `"an \`if\` in expression position …"` → `"when \`if\` is used as a value, each branch must end with a value"`
- `Parser/Items.rs:993`: `"after the single-expression function body"` (internal expect, less visible)
- `Parser/mod.rs:180`: `"split the expression into smaller steps"` → `"split this into smaller steps with \`val\` bindings"`

· **Risk:** Every one of these strings reaches user output. They break the product-copy
contract and the snapshot-pinning model (I4 + diagnostics.md).
· **Fix:** Rewrite each per the alternatives above. Bless new snapshots.
· **Confidence:** High

---

**CLARITY-2** · `Source/REPL.rs:450`
· **Internal error message leaks "parse error" to user**
· `format!("error: materialization parse error: {}\n", …)` — "parse error" is banned
voice (diagnostics.md). This fires when REPL's comptime materialization fails; users
shouldn't see compiler jargon.
· **Risk:** Low probability, but violates voice consistency when triggered.
· **Fix:** `"error: could not build this value: {}\n"` or similar plain-language text.
· **Confidence:** High
