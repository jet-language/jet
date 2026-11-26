# Power Asserts & Structural `assert_eq` Diffs — Design Review

- **Date:** 2026-11-26
- **Scope:** Full mechanism review for power `assert` subexpression alignment and structural `assert_eq` diffs across structs, lists, maps, and text (zero new syntax, zero overhead on the passing path, tier parity across `O0`, `O1`, and `O2`).

---

## 1. Current State Audit — Every Place `assert`, `assert_eq`, and Test-Diff Rendering Live Today

Every assertion and test-diff surface in the repository was audited before designing the cutover:

| Layer | File(s) & Symbols | Current Behavior | Gap vs Power Asserts / Structural Diff |
| :--- | :--- | :--- | :--- |
| **Prelude / Surface Docs** | `Compiler/Prelude/Core/prelude.jet:17-18` | Documents ambient builtins `assert(cond: Bool[, msg: String])` and `assert_eq(a, b)`. | No syntax change needed; already registered as ambient builtins (`Tools/Editors/jet.tmGrammar:305`, `Tools/Editors/Vscode/Syntaxes/jet.tmLanguage.json:305`, `Tools/Editors/TreeSitter/grammar.js:23`, `Tools/Editors/Zed/languages/jet/highlights.scm:98`). |
| **Type Checker (`JetSema`)** | `Compiler/JetSema/Source/Sema/Calls/Direct.jet:377-378, 992-1059` (`sema_call_check_ambient_assert`, `sema_call_check_ambient_assert_eq`), `Resolution.jet:148`, `Expressions/Control.jet:1762`, `ScopeMembers.jet:383`, `Tree.jet:334, 539-541`, `BodyFlatten.jet:839-843` | Type-checks `assert` into `TExprKind.RequireStop{kind: TRequireKind.Require{cond, msg}, location, always_stops}` and `assert_eq` into `TRequireKind.RequireEq{left, right, hook}`. Every subexpression of `cond` already preserves its checked `Type` and source `Span` in `BodyTree` (`BodyNode.span`, `BodyNode.ty`). | Already preserves all type and span facts needed for power asserts and structural diffs. **No Sema AST/IR schema change or new syntax is required.** |
| **IR Lowering (`JetIR`)** | `Compiler/JetIR/Source/BuildExpr.jet:9591-9782` (`ir_report_location`, `ir_require_stop`, `ir_require_eq_hook`, `ir_stop`), `BuildTypes.jet:1885-2360` (`ir_render`, `ir_render_push`, `ir_render_helper`), `BuildTypes.jet:3265-3530` (`ir_equal`, `ir_equal_into`, `ir_equal_helper`) | `ir_require_stop` evaluates `cond` into a single `Bool` and passes `"condition failed"` (or user `msg`) with `left = 0, right = 0` to `jet_rt_require_stop(0, descriptor, msg, 0, 0, locals)`. For `RequireEq`, it renders flat `Debug` strings `left_text` and `right_text` inside `if !equal` and passes `message = 0` to `jet_rt_require_stop(1, descriptor, 0, left_text, right_text, locals)`. | (1) Drops all intermediate subexpression values of `assert(cond)` — prints only `condition failed` + `locals:` (scalar locals only). (2) `assert_eq` prints flat `expected: ..., got: ...` with no path-level structural diff for structs, lists, maps, or text. |
| **Runtime Stop & Formatting (`JetRuntime`)** | `Compiler/JetRuntime/Source/Trap.jet:282-338, 1011-1090` (`trap_context`, `jet_rt_require_stop`, `trap_require_stop`, `trap_descriptor_context`, `trap_locals`), `TrapMessages.jet:416-424, 562`, `Compiler/JetFoundation/Source/Registry/RuntimeCExports.jet:2402-2403` | `trap_require_stop` prints `Stop: Jet.Panic: ...`, source context (`trap_context`), and `locals:` (`trap_locals`), gated by `!fault_frames_off()` so `--backtrace=off` stays frame-free. `left` is unused for `kind == 0`; `message` is unused for `kind == 1`. | Neither waterfall subexpression diagrams nor structural/text diffs are rendered by the runtime stop boundary. |
| **IR Evaluator (`JetEval`)** | `Compiler/JetEval/Source/IR/Native.jet:1143-1169` | Handles `X64_COMPTIME_STOP_REQUIRE` by reading `message` (`b`) for `a == 0` and `(left, right)` (`c, d`) for `a == 1`. | Keeping the existing 6-argument `jet_rt_require_stop(kind, descriptor, message, left, right, locals)` ABI preserves `JetEval`, `CABIDriver.c`, and `.expect_fail(message:)` raw message matching with zero breakage. |
| **Stdlib Testing (`Core/Testing`)** | `Core/Testing/Testing.jet:129-195, 1789-1913` (`snap`, `golden`, `testing_diff_lines`, `testing_unified_diff`) | Implements bounded LCS unified line diff (256 lines / 64 KiB input cap, 32 KiB output cap) specifically for `snap` and `golden` file-evidence failures recorded via `world_rt.record_failure`. | Only applies to `core.testing.snap` and `core.testing.golden` file comparisons; never invoked by ambient `assert` or `assert_eq`. |
| **Case Test Harness (`JetTestKit`)** | `Compiler/JetTestKit/Source/Expect.jet:8-27, 75-124` (`kit_expect_unified`, `kit_expect_exact`) | Host-only `.case` test runner comparing CLI stdout/stderr stream snapshots against `.expected`/`.out` files. | Separate host-side test harness for compiler integration `.case` files (not linked into user Jet programs). Must remain decoupled from runtime stops. |

---

## 2. Attacking the Mechanism — Hard Problems & Design Choices

### 2.1 Why Subexpression Capture Must Happen in `JetIR` (Not Sema Desugaring, Not Runtime Re-Evaluation)
1. **No re-evaluation / side-effect duplication:** Evaluating `assert(f() + g() == target)` or `assert(pop_next() > 0)` must call each subexpression **exactly once** in Jet's exact left-to-right short-circuiting order. Re-evaluating subexpressions on failure is unsound and forbidden.
2. **Zero formatting overhead on the passing path:** All `ir_render(..., IRRender.Debug, ...)` calls, `StringBuilder` allocations, diagram sorting, and structural diff walks live **strictly inside `if !condition { ... }` / `if !equal { ... }`**. On a passing assertion:
   - Unconditional subexpressions incur **zero extra allocations and zero formatting calls**; their already-computed IR slots simply stay alive until the `if !condition` branch closes (and fresh temporaries are released right after, performing the exact same single drop as before).
   - Conditional subexpressions inside short-circuiting `&&` / `||` or `CompareChain` store their value into a frame cell plus a 1-word `reached` flag only when evaluated — still zero heap allocations and zero formatting calls.
3. **SSA dominance across `O0`, `O1`, and `O2`:** Slots defined inside a short-circuit branch (`a && b`) do not dominate the post-condition failure block. By keeping unconditional captures in direct SSA slots and conditional captures in frame-allocated cells guarded by pre-initialized `reached` flags, every lowered function passes `ir_ssa_verify` and compiles identically under `-O0`, `-O1`, and `-O2`.
4. **Move safety & ownership:** A subexpression directly consumed by a `Move` node or a `^^` moving call argument cannot be borrowed after the move. Non-copyable moved subexpressions are excluded from post-failure capture (scalar copies remain safe), guaranteeing zero use-after-move and zero hidden copies.
5. **Preserving bare-literal & `--backtrace=off` behavior:**
   - Bare literals (`assert(false)`, `assert(false, "expected condition")`) have no non-literal subexpressions to annotate, so `detail = 0` and existing stop goldens remain byte-for-byte unchanged.
   - `trap_detail` checks `fault_frames_off()`, so `--backtrace=off` continues to emit only the stop line + `Why` / `Fix` / `More`.
   - `trap_require_stop` keeps `raw` identical (`message` for `kind == 0`, `"expected: <right>, got: <left>"` for `kind == 1`), so `.expect_fail(message:)` and task panic propagation stay 100% compatible.

### 2.2 Pixel-Exact Source Column Alignment
`trap_context` prints the source line as `<line> | <source_line>` (`digits` chars + `" | "` = `digits + 3` prefix columns before character column 1 of `<source_line>`). Therefore, each diagram line begins with `digits` spaces + `" | "` + `(col - 1)` alignment columns.

Subexpression anchor columns (1-based character columns on `report.source_line`):
- `Local`: start of identifier (`span.start`)
- `Field(_, member)`: start of `member` identifier after `.`
- `Binary(op, _, _, lhs, rhs)`: start of operator token between `lhs.span.end` and `rhs.span.start`
- `Unary`: operator token at `span.start`
- `Call` / `StaticCall`: start of callee expression
- `MethodCall` / `BuiltinMethod` / `NumericMethod` / `HandleMethod` / `ClosureMethod`: start of method name after `.`
- `Index`: `[` token between `base.span.end` and `index.span.start`
- Transparent wrappers (`Move`, `Borrow`, `Clone`, `ExplicitCopy`, `MaterializeView`, `DistinctCtor`, `DistinctRaw`) and bare literals (`IntLit`, `FloatLit`, `BoolLit`, `CharLit`, `StrLit`, `Unit`, `Absent`, `CtLit`) are not given redundant standalone columns.

Waterfall rendering (`jet_rt_assert_diagram` in `JetRuntime/Source/AssertDiff.jet`):
- Sorts reached `(col, text)` pairs ascending by `col`, deduplicating identical columns (keeping the outermost/compound result at that column).
- Emits one connector line with `|` at every captured column, then emits values rightmost-first (bottom-to-top) so no value text can collide with any vertical bar to its right.

### 2.3 Type-Driven Structural Diff for `assert_eq`
Rather than parsing `Debug` strings (which would break on custom `Debug` impls and lose map-key/field structure), `JetIR` already knows the exact compiled `Type` of `left` and `right` at `ir_require_stop`:
- **Structs** (via per-type keyed helper `#diff<row>`, mirroring `ir_render_helper` and `ir_equal_helper` so recursive structs terminate cleanly): compares every field in declaration order; child path is `field` at top level or `{path}.{field}` when nested.
- **Lists (`[T]`, `[T#N]`, views)**: compares indices `0..<min(len_l, len_r)` with child path `{path}[{i}]`, then reports trailing extra elements (`{path}[{i}]: extra <got>`) or missing elements (`{path}[{i}]: missing (expected <expected>)`).
- **Maps (`[K:V]`, both hashed and small entry-list maps)**: checks every `(k, lv)` in `left` against `right` (`extra` if missing in `right`, else recursive diff at `{path}[{debug(k)}]`), then checks every `(k, rv)` in `right` absent from `left` (`missing (expected <rv>)`).
- **Tuples / Options / Results / Enums**: recurses through named tuple fields (`.name`), positional tuple slots (`.0`), `Val`/`Ok`/`Err` payloads, and matching enum variant payloads; emits leaf difference when tags/variants differ.
- **Text (`String`)**: delegates to `jet_rt_assert_diff_text(builder, path, left, right, count_addr)`:
  - Multi-line strings: bounded LCS line diff reporting exact 1-based line numbers (`line N:` / `{path} (line N):`) with `- expected:` and `+ got:` lines.
  - Single-line strings: first differing Unicode scalar offset (`at char N:` / `{path} (at char N):`) with aligned caret under the first differing character at top level.
- **Plain top-level scalars (`Int`, `Float`, `Bool`, `Char`, `None`/`Val(scalar)`)**: emit `detail = 0`, preserving the concise `expected: ..., got: ...` stop with zero redundant diff block.
- **Bounded output**: caps reported leaf differences at 32 entries with a `... (more differences omitted)` footer so large collections never hang or blow memory.

### 2.4 Zero-ABI-Breakage Wire Format
`jet_rt_require_stop(kind, descriptor, message, left, right, locals)` keeps its exact 6-word C ABI:
- `kind == 0` (`assert`): `message` = stop message (`"condition failed"` or user message); `left` = aligned power-assert waterfall diagram string handle (or `0`); `right` = `0`.
- `kind == 1` (`assert_eq`): `message` = structural diff string handle (or `0` for plain scalars); `left` = `Debug(got)`; `right` = `Debug(expected)`.
- `kind == 2` (`panic`) and `kind == 3` (contract): untouched.
- `test_scope_pending_failure()` (`snap`/`golden`/`fixture`): passes `left = 0, right = 0`, untouched.

---

## 3. Exact Cutover List & Deleted Copies

### Files Added
- `Docs/Audits/2026-11-26-power-asserts-review.md` — this design review.
- `Compiler/JetRuntime/Source/AssertDiff.jet` — single runtime owner of `jet_rt_assert_diagram` (waterfall column alignment) and `jet_rt_assert_diff_text` (bounded LCS multi-line diff and single-line scalar-offset diff).
- `Compiler/JetIR/Source/BuildAssert.jet` — single IR owner of power-assert subexpression collection/capture and `#diff<row>` structural diff lowering.
- `Compiler/JetIR/Tests/PowerAssert.jet` — IR unit tests verifying zero passing-path formatting, short-circuit SSA dominance, and structural diff IR emission.
- `Compiler/JetRuntime/Tests/AssertDiff.jet` — unit tests for waterfall alignment, deduplication, multi-line text diff, and single-line char-offset diff.
- `Examples/Tooling/PowerAssert/PowerAssert.jet` — executable golden example exercising passing and failing `assert` / `assert_eq` across structs, lists, maps, and text.
- `Tests/Cases/JetTest/power_assert_and_structural_diff.case` — end-to-end case test proving `O0`, `O1`, and `O2` parity for power `assert` alignment and `assert_eq` structural diffs on structs, lists, maps, and text.

### Files Modified
- `Compiler/JetFoundation/Source/Registry/RuntimeCExports.jet` — register `jet_rt_assert_diagram` and `jet_rt_assert_diff_text`.
- `Compiler/JetRuntime/sources.list` — include `Compiler/JetRuntime/Source/AssertDiff.jet`.
- `Compiler/JetRuntime/Source/Trap.jet` — add `trap_detail` and print `left` (`kind == 0`) / `message` (`kind == 1`) in `trap_require_stop` gated by `!fault_frames_off()`.
- `Compiler/JetIR/Source/Build.jet` — add capture tracking fields (`assert_captures`, `assert_held_fresh`) to `IRBuilder`.
- `Compiler/JetIR/Source/BuildExpr.jet` — hook `ir_expr` / `ir_binary_chain` capture recording (no-op when inactive) and invoke `ir_assert_lower` / `ir_assert_eq_diff` inside `ir_require_stop`.
- `Tests/Cases/BuildEntry/stops_follow_the_backtrace_option.assert.brief.stderr` — update expected `assert(n < 2, "n stays small")` output to include the aligned power-assert subexpression diagram (`.assert.off.stderr` and `.assert_eq.*.stderr` stay unchanged).
- `Docs/Spec/spec.md` & `Docs/Spec/Reference/` — document power `assert` subexpression diagrams and structural `assert_eq` diffs across structs, lists, maps, and text.

### Copies Deleted / Duplication Avoided
- **No duplicate expression evaluator:** Subexpression capture hooks directly into `ir_expr` / `ir_binary_chain` instead of duplicating expression lowering in a second walker.
- **No duplicate `Debug` parser:** Structural diff lowers directly from `JetIR`'s type layout helpers (`ir_struct_members`, `ir_map_shape`, `ir_sequence_element`, `ir_render`), sharing `ir_equal` and `ir_render` rather than re-parsing printed strings.
- **No second `JetTestKit` / `Core/Testing` copy:** `JetTestKit/Source/Expect.jet` remains strictly host `.case` snapshot diffing, and `Core/Testing/Testing.jet` remains file-evidence `snap`/`golden` diffing; runtime assertion formatting and structural text diffing have one owner (`JetRuntime/Source/AssertDiff.jet`).

---

## 4. Residual Gaps & Bounded Non-Goals
- **Multi-line `assert(...)` conditions:** Subexpressions on lines other than `report.source_line` are outside the printed single-line source snippet and are omitted from the aligned waterfall (the top-level failure and `locals:` still print).
- **Moved non-copyable temporaries inside `assert(...)`:** A non-copyable value moved into a consuming function inside the condition cannot be rendered after the move without inserting a hidden copy; we intentionally omit it to uphold zero hidden copies on the passing path.
- **Opaque foreign/host handles and closures:** Values without structural fields (handles, function values) fall back cleanly to their `Debug` rendering in `expected: ..., got: ...` without crashing or fabricating paths.
- **Large collection diffs:** Capped at 32 leaf differences (and 256 lines for multi-line text) to guarantee bounded failure latency and memory usage.
