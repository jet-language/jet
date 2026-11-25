# Addendum: a fresh review of `^` and `^^` as the write and take marks

Date: 2026-11-25. Card: #6998, criterion 1. Adds to
`2026-11-24-caret-marks-adversarial-review.md`, which stays as written.

A second reviewer, who did not write the first review or the ballot draft,
attacked the shipped spelling again. Probes are single files under
`$JET_SCRATCH_ROOT/g1gen3-probe/`, run with the batch-41f and batch-42 stage
compilers against the tree at `9f92ea706b2`.

## New findings

### The take-to-write collapse is silent

The first review predicted that a dead-key layout turns `^^self` into
`^self`. A probe now shows the cost. `Ticket` has
`redeem: fn(^self) -> Int { self.id }`, the shape the collapse leaves, and
`run` calls `t.redeem()` twice. It checks clean and prints `7 7`: the ticket
was meant to be used up and was redeemed twice. With `^^self` the same
program is refused with `Jet.UseAfterMove`. So the collapse is caught only
when the body moves the receiver, and no diagnostic names the dead key.

Proposed mitigation (new surface, sent to the orchestrator as a criterion
need): flag a write-marked parameter or receiver that the body never writes,
as `Jet.MutableNeverReassigned` flags a `:=` binding that never changes. That
catches the collapse in the probe, since `redeem` never writes `self`.

### Composed letters: the help covers type names only

- `fn(x: Înt)` gets E0119 naming the dead key, then a follow-on
  `Jet.ImmutableAssignment` that advises `x := ...` for a parameter.
- `fn(êntries: [Int])` compiles and runs silently.
- `fn(ŝelf)` is told "Write `ŝelf: Type`", advice that keeps the typo.
- `if ŝhape == {` and `ĉounts` get only a spelling suggestion.
- `fn(x: ĵson.Dyn)` after `use json: core.encoding.json` panics the
  compiler: `can't slice 8 characters from 6 to 9`, from
  `sema_registration_unit_leaf_name` in
  `Compiler/JetSema/Source/Sema/Registration/Items.jet`, which slices a
  string by character with an end from `count_bytes()`. Any non-ASCII
  qualified type name can reach it; dead keys make it likely.

The E0119 help lives in `sema_dead_key_base` and its callers in
`Compiler/JetSema/Source/Sema/FailureLaw.jet`, reached only from
`sema_unknown_type_name`. Criterion 6 owns the remaining hints.

### Readers from other languages

Go reads a prefix `^x` as bitwise complement, so `edit: ^values[1..<3]` can
read as a value rather than a write window. C, Rust and Swift read infix `^`
as exclusive-or; in Jet `c: a ^ b` is a parse error ("found ^") and
`print(x ^ 3)` asks for a comma. `a ^= 3` is reported as "This line computes
a value but doesn't do anything with it", because the lexer still makes a
`^=` token (`TOKEN_CARET_EQ`) that no rule reads.

### Tools

- The tree-sitter grammar had no `^^` token, so `^^` lexed as two `^`, and
  the Zed highlight query still colored `&&` (logical and) as the take mark.
  A guard pinned the stale `&&` queries. Fixed in the criterion 4 patch.
- The comptime token-kind names in `Compiler/JetEval/Source/CompilerServices.jet`
  call the marks `operator.caret` and `operator.caret_caret`, though neither
  is an operator.
- The diagnostic underline is also `^`. It sits on its own gutter line under
  a marked type (`fn(x: ^Intt)`), which reads acceptably.

## Verdicts

| Claim | Verdict |
| --- | --- |
| Marks and operators are apart | Holds: `&` and `&&` now mean only bitwise and logical and |
| The marks read well | Holds within limits: Go, Odin and Pascal readers bring other meanings |
| The marks are easy to type | Fails on dead-key layouts: a take silently becomes a write |
| No lasting diff noise | Holds within limits: one migration commit, skipped with `git blame --ignore-rev` |
| Tooling handles the marks | Holds within limits: the tree-sitter and Zed gaps are fixed; the `^=` token and comptime names remain |
