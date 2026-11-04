# G15 probes: results

Compiler: the dev build of 624ba98eccbce9794f7e9298cfad9b168f844800 (`jet` from the discovery base). Each probe ran from `Docs/audits/g15-2026-11/probes/`.
`run` is `jet run FILE`; `release` is `jet run --release FILE`; `check` is `jet check FILE`; `complexity` is `jet lint --complexity FILE`.
The host has no rustc, cargo, cc, clang or ld on PATH.

## c3590_9_loose_dep.jet (#3590 c9)

`jet run c3590_9_loose_dep.jet` exit 1

```
-- stderr
Error [jet.UnresolvedInlineDependency]: Inline dependency `textkit#1.4` didn't resolve
  --> probes/c3590_9_loose_dep.jet:2:12
    |
  2 | use textkit#1.4
    |            ^^^^
 Why: No local source knows a package named `textkit` — an inline dependency must resolve from a committed or explicitly materialized local copy.
 Fix: Commit a copy at `.jet/inline-deps/textkit/<version>/`, materialize the source locally first, or run `jet init` and depend on `textkit` through `package.jet`.
More: jet-lang.dev/e/E1253
```

## c4717_11_imports.jet (#4717 c11)

`jet run c4717_11_imports.jet` exit 0

```
15 Core modules imported
```

`jet run --release c4717_11_imports.jet` exit 0

```
15 Core modules imported
```

## c4717_42_atomic_field.jet (#4717 c42)

`jet run c4717_42_atomic_field.jet` exit 0

```
prior=5 now=7
9
```

`jet run --release c4717_42_atomic_field.jet` exit 0

```
prior=5 now=7
9
```

## c4717_42_atomic_field_bad.jet (#4717 c42)

`jet run c4717_42_atomic_field_bad.jet` exit 1

```
-- stderr
Error [jet.BindingTypeMismatch]: This needs Int, but the value is String
  --> probes/c4717_42_atomic_field_bad.jet:7:32
    |
  7 |     counters :: Counters{hits: "five"}
    |                                ^^^^^^
 Why: The value here does not have the type required by this declaration
 Fix: Use a value of the required type or convert it explicitly
More: jet-lang.dev/e/E0108

1 problem found
run `jet explain E0108` to learn more
```

`jet run --release c4717_42_atomic_field_bad.jet` exit 1

```
-- stderr
Error [jet.BindingTypeMismatch]: This needs Int, but the value is String
  --> probes/c4717_42_atomic_field_bad.jet:7:32
    |
  7 |     counters :: Counters{hits: "five"}
    |                                ^^^^^^
 Why: The value here does not have the type required by this declaration
 Fix: Use a value of the required type or convert it explicitly
More: jet-lang.dev/e/E0108

1 problem found
run `jet explain E0108` to learn more
```

## c4717_46_dropped_pure.jet (#4717 c46)

`jet check c4717_46_dropped_pure.jet` exit 1

```
-- stderr
Error [jet.ValueThrownAway]: The result of `double` is thrown away
  --> c4717_46_dropped_pure.jet:7:5
    |
  7 |     double(2)
    |     ^^^^^^
 Why: `double` is pure (it has no effects and changes nothing through `&`), so this line does nothing
 Fix: Use the result, bind it with `result :: double`, or write `.drop("reason")` if dropping it is intended
More: jet-lang.dev/e/E0433

1 problem found
run `jet explain E0433` to learn more
```

## c4717_73_complexity.jet (#4717 c73)

`jet lint --complexity c4717_73_complexity.jet` exit 0

```
c4717_73_complexity.jet:2 fn two_loops — score 3
c4717_73_complexity.jet:21 fn if_in_loop — score 3
c4717_73_complexity.jet:12 fn nested_if — score 2
c4717_73_complexity.jet:31 fn run — score 0
architecture advice (advisory, operator may reject):
  consumer-agreement `run` [same-module]
    evidence: callee-definition c4717_73_complexity.jet:101..110
    evidence: callee-definition c4717_73_complexity.jet:247..256
    evidence: callee-definition c4717_73_complexity.jet:359..369
    evidence: caller-definition c4717_73_complexity.jet:498..501
    evidence: checked-call-site c4717_73_complexity.jet:516..525
    evidence: checked-call-site c4717_73_complexity.jet:541..550
    evidence: checked-call-site c4717_73_complexity.jet:565..575
    boundary: c4717_73_complexity.jet -> c4717_73_complexity.jet
    boundary: effects run: IO
    check: checked_reference_resolution=passed (the compiler semantic index resolved the definitions and call sites)
    check: consumer_behavior=not_run (this read-only inspection does not execute user code)
    check: cross_mode_agreement=unknown (no runtime or change-task receipt was supplied)
    uncertainty: one checked consumer joins multiple same-result call paths; review whether their results must agree
    uncertainty: the checked call graph proves references, not a business invariant
    uncertainty: operator review may reject this candidate; no refactor or policy is applied
  measurement: benchmark/change-task not measured; no advantage claim
```

## c4717_78_slice_view.jet (#4717 c78)

`jet run c4717_78_slice_view.jet` exit 1

```
-- stderr
Error [jet.ReturnTypeMismatch]: This value is String (text), but `prefix` returns View<str>
  --> probes/c4717_78_slice_view.jet:3:10
    |
  3 |     text.slice(0..<3)
    |          ^^^^^
 Why: `prefix` declares View<str> as its result, so the value it hands back must be View<str>
 Fix: Hand back a View<str> value: convert this value to View<str>, or return the value you meant
More: jet-lang.dev/e/E0113

1 problem found
run `jet explain E0113` to learn more
```

`jet run --release c4717_78_slice_view.jet` exit 1

```
-- stderr
Error [jet.ReturnTypeMismatch]: This value is String (text), but `prefix` returns View<str>
  --> probes/c4717_78_slice_view.jet:3:10
    |
  3 |     text.slice(0..<3)
    |          ^^^^^
 Why: `prefix` declares View<str> as its result, so the value it hands back must be View<str>
 Fix: Hand back a View<str> value: convert this value to View<str>, or return the value you meant
More: jet-lang.dev/e/E0113

1 problem found
run `jet explain E0113` to learn more
```

## c4717_79_move_across_view.jet (#4717 c79)

`jet run c4717_79_move_across_view.jet` exit 1

```
-- stderr
Error [jet.OwnerMovedWhileViewed]: `owner` cannot be moved while `view` is still looking into it
  --> probes/c4717_79_move_across_view.jet:9:14
    |
  9 |     consume(^owner)
    |              ^^^^^
 Why: `view` is a live read view into `owner`; changing or moving the owner could invalidate that view
 Fix: Finish using `view` before changing `owner`, narrow the view's scope, or make an owned copy
More: jet-lang.dev/e/E0212

1 problem found
run `jet explain E0212` to learn more
```

`jet run --release c4717_79_move_across_view.jet` exit 1

```
-- stderr
Error [jet.OwnerMovedWhileViewed]: `owner` cannot be moved while `view` is still looking into it
  --> probes/c4717_79_move_across_view.jet:9:14
    |
  9 |     consume(^owner)
    |              ^^^^^
 Why: `view` is a live read view into `owner`; changing or moving the owner could invalidate that view
 Fix: Finish using `view` before changing `owner`, narrow the view's scope, or make an owned copy
More: jet-lang.dev/e/E0212

1 problem found
run `jet explain E0212` to learn more
```

## c4717_81_payload_root.jet (#4717 c81)

`jet run c4717_81_payload_root.jet` exit 0

```
2
[7, 8]
[7, 8]
```

`jet run --release c4717_81_payload_root.jet` exit 0

```
2
[7, 8]
[7, 8]
```

## c4717_81_temporary_refused.jet (#4717 c81)

`jet run c4717_81_temporary_refused.jet` exit 0

```
[7, 132095651680912]
```

`jet run --release c4717_81_temporary_refused.jet` exit 0

```
[7, 136508477985424]
```

## c4717_75_write_after_lambda_copy.jet (#4717 c75)

`jet check c4717_75_write_after_lambda_copy.jet` exit 0

```
-- stderr
ok: `c4717_75_write_after_lambda_copy.jet` has no problems
```

`jet run c4717_75_write_after_lambda_copy.jet` exit 0

```
1
2
```

## c4717_82_bare_param_view.jet (#4717 c82)

`jet run c4717_82_bare_param_view.jet` exit 1

```
-- stderr
Error [jet.ReturnTypeMismatch]: This value is [Int], but `all` returns View<Int>
  --> probes/c4717_82_bare_param_view.jet:7:5
    |
  7 |     items
    |     ^^^^^
 Why: `all` declares View<Int> as its result, so the value it hands back must be View<Int>
 Fix: Hand back a View<Int> value: convert this value to View<Int>, or return the value you meant
More: jet-lang.dev/e/E0113

Error [jet.ReturnTypeMismatch]: This value is [Int], but `field` returns View<Int>
  --> probes/c4717_82_bare_param_view.jet:11:12
    |
 11 |     holder.items
    |            ^^^^^
 Why: `field` declares View<Int> as its result, so the value it hands back must be View<Int>
 Fix: Hand back a View<Int> value: convert this value to View<Int>, or return the value you meant
More: jet-lang.dev/e/E0113

2 problems found
run `jet explain E0113` to learn more
```

`jet run --release c4717_82_bare_param_view.jet` exit 1

```
-- stderr
Error [jet.ReturnTypeMismatch]: This value is [Int], but `all` returns View<Int>
  --> probes/c4717_82_bare_param_view.jet:7:5
    |
  7 |     items
    |     ^^^^^
 Why: `all` declares View<Int> as its result, so the value it hands back must be View<Int>
 Fix: Hand back a View<Int> value: convert this value to View<Int>, or return the value you meant
More: jet-lang.dev/e/E0113

Error [jet.ReturnTypeMismatch]: This value is [Int], but `field` returns View<Int>
  --> probes/c4717_82_bare_param_view.jet:11:12
    |
 11 |     holder.items
    |            ^^^^^
 Why: `field` declares View<Int> as its result, so the value it hands back must be View<Int>
 Fix: Hand back a View<Int> value: convert this value to View<Int>, or return the value you meant
More: jet-lang.dev/e/E0113

2 problems found
run `jet explain E0113` to learn more
```

## c4717_83_fixed_range_view.jet (#4717 c83, c84)

`jet run c4717_83_fixed_range_view.jet` exit 0

```
[2, 3]
5
11
```

`jet run --release c4717_83_fixed_range_view.jet` exit 0

```
[2, 3]
5
11
```

## c4717_79_read_borrow_kept.jet (#4717 c79)

`jet run c4717_79_read_borrow_kept.jet` exit 0

```
9
true
stream
```

`jet run --release c4717_79_read_borrow_kept.jet` exit 0

```
9
true
stream
```

## c4717_85_swap_loop.jet (#4717 c85)

`jet run c4717_85_swap_loop.jet` exit 0

```
[2]
[1]
```

`jet run --release c4717_85_swap_loop.jet` exit 0

```
[2]
[1]
```

## c4717_85_swap_loop_unreinit.jet (#4717 c85)

`jet run c4717_85_swap_loop_unreinit.jet` exit 1

```
-- stderr
Error [jet.UseAfterMove]: `a` was consumed by the move `^a`, so it can't be used here
  --> probes/c4717_85_swap_loop_unreinit.jet:5:15
    |
  5 |         t :: ^a
    |               ^
  --> probes/c4717_85_swap_loop_unreinit.jet:5:14
    |
  5 |         t :: ^a
    |              ^^ `a` moved to the move `^a` here on an earlier loop iteration
 Why: After a value moves somewhere else, its old name no longer gives access to it
 Fix: Stop using `a` after the move, or give the move `^a` a copy with `a.copy()` where it moved.
More: jet-lang.dev/e/E0121

1 problem found
run `jet explain E0121` to learn more
```

`jet run --release c4717_85_swap_loop_unreinit.jet` exit 1

```
-- stderr
Error [jet.UseAfterMove]: `a` was consumed by the move `^a`, so it can't be used here
  --> probes/c4717_85_swap_loop_unreinit.jet:5:15
    |
  5 |         t :: ^a
    |               ^
  --> probes/c4717_85_swap_loop_unreinit.jet:5:14
    |
  5 |         t :: ^a
    |              ^^ `a` moved to the move `^a` here on an earlier loop iteration
 Why: After a value moves somewhere else, its old name no longer gives access to it
 Fix: Stop using `a` after the move, or give the move `^a` a copy with `a.copy()` where it moved.
More: jet-lang.dev/e/E0121

1 problem found
run `jet explain E0121` to learn more
```

## c4717_87_fresh_capture.jet (#4717 c87)

`jet run c4717_87_fresh_capture.jet` exit 0

```
0
1
2
```

`jet run --release c4717_87_fresh_capture.jet` exit 0

```
0
1
2
```

## c4717_89_lambda_result.jet (#4717 c89)

`jet check c4717_89_lambda_result.jet` exit 1

```
-- stderr
Error [jet.LambdaReturnType]: A lambda cannot write a return or error type
  --> c4717_89_lambda_result.jet:5:26
    |
  5 |     print(apply((n: Int) Int -> n * 2))
    |                          ^^^
 Why: A lambda's arrow introduces its body; return and error types come from the expected callable type
 Fix: Remove the type; pass the lambda to a typed parameter or slot, or use a named `fn` for a full interface
More: jet-lang.dev/e/E0399

1 problem found
run `jet explain E0399` to learn more
```

## c4717_90_pre_arrow.jet (#4717 c90)

`jet check c4717_90_pre_arrow.jet` exit 1

```
-- stderr
Error [jet.MissingFunctionBody]: `fn f` has no body
  --> c4717_90_pre_arrow.jet:2:8
    |
  2 | fn f() Int -> 1
    |        ^^^
 Why: Jet functions are defined, not declared: a signature without `{ … }` is not a complete function. Host and FFI cells use `#Import` / `#FFI` with their required body form.
 Fix: Add a `{ … }` block.
More: jet-lang.dev/e/E0081

Error [jet.InvalidStatement]: This line computes a value but doesn't do anything with it
  --> c4717_90_pre_arrow.jet:2:8
    |
  2 | fn f() Int -> 1
    |        ^^^^^^^^
 Why: Only calls, bindings, assignments, and `return` are allowed here
 Fix: Use the value, e.g. `x :: ...` or `print(...)`
More: jet-lang.dev/e/E0003

2 problems found
run `jet explain E0081` to learn more
```

## c4717_93_incdec.jet (#4717 c93)

`jet check c4717_93_incdec.jet` exit 1

```
-- stderr
Error [jet.InvalidStatement]: This line computes a value but doesn't do anything with it
  --> c4717_93_incdec.jet:4:5
    |
  4 |     n++
    |     ^
 Why: Only calls, bindings, assignments, and `return` are allowed here
 Fix: Use the value, e.g. `x :: ...` or `print(...)`
More: jet-lang.dev/e/E0003

1 problem found
run `jet explain E0003` to learn more
```

## c4717_94_bidi.jet (#4717 c94)

`jet check c4717_94_bidi.jet` exit 0

```
-- stderr
ok: `c4717_94_bidi.jet` has no problems
```

## c4599_28_comptime_asm.jet (#4599 c28)

`jet run c4599_28_comptime_asm.jet` exit 0

```
42
42
```

`jet run --release c4599_28_comptime_asm.jet` exit 0

```
42
42
```

## c4599_28_comptime_asm_reach.jet (#4599 c28)

`jet run c4599_28_comptime_asm_reach.jet` exit 1

```
-- stderr
Error [jet.MissingBlockValue]: This block promises to produce Int, but its last line produces no value
  --> probes/c4599_28_comptime_asm_reach.jet:10:1
    |
 10 | #[Unsafe("the operands are scalar registers and the early-written r10 clobber does not overlap live inputs"), FFI(asm)]
    | ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
 Why: A block that promises Int must end with a value of that type, or leave through `return` on every path
 Fix: End the block with an expression that produces Int, or `return` a value on every path
More: jet-lang.dev/e/E0114

1 problem found
run `jet explain E0114` to learn more
```

`jet run --release c4599_28_comptime_asm_reach.jet` exit 1

```
-- stderr
Error [jet.MissingBlockValue]: This block promises to produce Int, but its last line produces no value
  --> probes/c4599_28_comptime_asm_reach.jet:10:1
    |
 10 | #[Unsafe("the operands are scalar registers and the early-written r10 clobber does not overlap live inputs"), FFI(asm)]
    | ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
 Why: A block that promises Int must end with a value of that type, or leave through `return` on every path
 Fix: End the block with an expression that produces Int, or `return` a value on every path
More: jet-lang.dev/e/E0114

1 problem found
run `jet explain E0114` to learn more
```

## c4599_28_runtime_asm_reach.jet (#4599 c28 (control))

`jet run c4599_28_runtime_asm_reach.jet` exit 0

```
42
201
```

`jet run --release c4599_28_runtime_asm_reach.jet` exit 0

```
42
201
```

## c4599_102_next_in_value_table.jet (#4599 c102)

`jet run c4599_102_next_in_value_table.jet` exit 1

```
-- stderr
Error [jet.UnknownName]: Nothing named `next` exists here
  --> probes/c4599_102_next_in_value_table.jet:6:18
    |
  6 |             0 -> next
    |                  ^^^^
 Why: A name must be declared before it's used
 Fix: Declare it first: `next :: ...`
More: jet-lang.dev/e/E0107

1 problem found
run `jet explain E0107` to learn more
```

`jet run --release c4599_102_next_in_value_table.jet` exit 1

```
-- stderr
Error [jet.UnknownName]: Nothing named `next` exists here
  --> probes/c4599_102_next_in_value_table.jet:6:18
    |
  6 |             0 -> next
    |                  ^^^^
 Why: A name must be declared before it's used
 Fix: Declare it first: `next :: ...`
More: jet-lang.dev/e/E0107

1 problem found
run `jet explain E0107` to learn more
```

## c4599_103_literal_subpattern.jet (#4599 c103)

`jet run c4599_103_literal_subpattern.jet` exit 0

```
dot
circle 5
unit square
square 4
label x
other
```

`jet run --release c4599_103_literal_subpattern.jet` exit 0

```
dot
circle 5
unit square
square 4
label x
other
```

## c3590_cli.sh (#3590 c7, c8, c11)

`bash probes/c3590_cli.sh DIR` exit 0

```
jet add textkit -> exit 2
jet fetch -> exit 2
jet build --locked -> exit 2
created newp/
  package.jet
  run.jet
  .gitignore
next: cd newp && jet run
Error [jet.InvalidCommandInput]: `jet add` is not built yet
 Why: This `jet` has no implementation of this command form yet, and no other `jet` can run it.
 Fix: Use a form this `jet` supports (`jet help` lists them); this one needs its Jet implementation first.
More: jet-lang.dev/e/E2104
Error [jet.InvalidCommandInput]: `jet fetch` is not built yet
 Why: This `jet` has no implementation of this command form yet, and no other `jet` can run it.
 Fix: Use a form this `jet` supports (`jet help` lists them); this one needs its Jet implementation first.
More: jet-lang.dev/e/E2104
Error [jet.InvalidCommandInput]: `jet build --locked` is not built yet
 Why: This `jet` has no implementation of this command form yet, and no other `jet` can run it.
 Fix: Use a form this `jet` supports (`jet help` lists them); this one needs its Jet implementation first.
More: jet-lang.dev/e/E2104
```

