# Adversarial Design Review: One Checking-Context Record for Sema (`#6999 E`)

**Date:** `2026-11-24`  
**Base:** dev `68cf40181c`  
**Card:** `#6999` criterion E ("one checking-context record replaces the threaded environment/program/scopes/state/tail_store/context/diagnostics bundle")

## Verdict: **FAILS as proposed; HOLDS narrowed**

A record that holds all seven parts cannot be built without copying the
environment and the program tables on every body check, and no record can be
introduced one area at a time. A record of the three parts each body check
creates for itself (ownership state, statement tail store, check context)
holds. It costs no copy and leaves collect-all unchanged, but it must switch in
one change across the whole checker.

## 1. The design under review

```jet
SemaCheck: struct {
    environment: SemaScopeEnvironment,
    program: SemaProgramTables,
    scopes: SemaScopeTable,
    state: SemaOwnershipState,
    tail_store: SemaStatementTailStore,
    context: SemaCheckContext,
    diagnostics: [Diagnostic],
}
```

Every checker function would take `check: ^SemaCheck` in place of the bundle
`environment: SemaScopeEnvironment, program: SemaProgramTables, scopes:
^SemaScopeTable, state: ^SemaOwnershipState, tail_store:
^SemaStatementTailStore, context: ^SemaCheckContext, diagnostics:
^[Diagnostic]`. Here is how access would work:

- A function that reads a part reads `check.environment.module_identity`.
- A function that writes a part writes `check.scopes.loop_depth = 0`.
- A callee that needs only some parts gets field borrows:
  `sema_scope_enter(check.scopes, check.state, key)`.
- A body entry builds the record with `^^` moves and takes the parts back out
  with `.move()`.

## 2. Measured facts (JetSema on the base)

| Fact | Count |
|---|---|
| Signatures with the exact seven-part bundle | 466 |
| Functions taking `state`, `tail_store` and `context` | 540 |
| ...of which form one strongly connected call component | 520 |
| ...root functions no other bundle function calls | 8 |
| Call sites passing `state, tail_store, context` | 1,583 |
| Places a body check creates a tail store (`sema_statement_tail_store_init()`) | 6 (`CheckProgram.jet` x2, `MarkerRules.jet`, `Expressions/NumberNames.jet` x3) |
| Parameter occurrences in Compiler/: `program` read / `diagnostics` / `environment` read / `state` / `scopes` / `context` / `tail_store` | 1,692 / 1,610 / 1,506 / 831 / 786 / 771 / 555 |

Probes were run with the batch28 compiler against this base:

- **P1. Field borrows:** one `^` record lends a read part and two write parts
  to one call (`read_and_write(checker.environment, checker.scopes,
  checker.diagnostics)`). The ownership checker accepts it and the program
  runs correctly. A record lends its parts without copying them.
- **P2. Borrowed field:** a field typed `environment: ^Env` is a parse error:
  `Jet.InvalidStatement: Expected a type, found ^`. A record cannot hold a
  borrow. Every part must be owned.
- **P3. Moving a part out of a borrow:** building a record from a `^`
  parameter (`Check{scopes: scopes.move(), ...}`) is refused with
  `Jet.MissingMoveMarker: scopes cannot be moved from borrowed storage`. Core
  has no swap or take primitive. A function that holds borrowed parts cannot
  lend them to a callee that takes the record.
- **P4. Compile speed:** I generated two programs, each a chain of 400
  functions. In one, each function takes the seven parts. In the other, each
  takes `environment, program, scopes, check: ^Check, diagnostics`, where the
  record holds three parts. Seven runs each on a shared machine:

  | | `jet check` median | `jet build` median |
  |---|---|---|
  | Seven parts | 211 ms | 1,961 ms |
  | Record | 283 ms | 1,903 ms |

  The run-to-run spread (122 to 330 ms for check, 1,032 to 2,538 ms for build)
  is larger than either difference. Neither form changes compile speed
  measurably.

## 3. Attacks

**A1. Copy cost: fails for `environment` and `program`.**
- Every body check borrows both parts as read parameters.
  `sema_function_check_pass` receives them read, and
  `sema_check_comptime_item_in` borrows the environment so that reading its
  registration graph never copies it.
- Body threads share the program tables read-only. Each thread reads the
  module scope in place through `sema_scope_light_copy`.
- P2 rules out a borrowed field, so an owning record needs a copy of both parts
  per body check.
- **Verdict:** they stay read parameters. A read parameter costs nothing.

**A2. Copy cost: fails for `scopes` and `diagnostics`.**
- The scope table a body check writes is lent by its caller, for example
  `comptime_scopes.slots[at].table`, and it outlives the check.
- The diagnostics list collects findings across bodies. Callers keep marks into
  it (`diagnostics_start: diagnostics.len()`) and rewrite spans after the check.
- By P3, neither can move into a record without restructuring every lender into
  a pop-and-push of its slot.
- **Verdict:** both stay `^` parameters.

**A3. Copy cost: holds for `state`, `tail_store` and `context`.**
- Every body entry creates all three as locals: `sema_ownership_state_init()`,
  `sema_statement_tail_store_init()` and `sema_check_context_init()`. They live
  only for that check.
- Moving them into a record at the entry is free, and P1 shows callees can
  borrow them back as fields.
- No new `.copy()` is needed, so the copy ratchet cannot rise.

**A4. Ownership-checker limits: holds, with one rule.**
- P1 shows disjoint field borrows of one `^` record pass in a single call.
- A call that passes both the record and one of its own fields overlaps
  borrows. Rule: a callee takes either the record or some of its parts, never
  both.
- `SemaCheckContext` cannot absorb `state` and `tail_store` as fields of its
  own. Callers that pass `state` and `context` together would then pass
  `context.state, context`, which overlap. The three parts must be siblings in
  a new record.

**A5. Migration: the plan to switch one area at a time fails.**
- Converting a function to the record requires every caller to hold a record.
- By P3, a caller holding borrowed parts cannot build one. It can only copy.
- 520 of the 540 functions form one strongly connected component through
  `sema_check_expression`, `sema_check_statement` and the Calls/ dispatch.
  Every member therefore has a caller inside the component.
- Only the 8 roots and the 20 functions outside the component could convert
  early, top-down.
- The switch is one change: about 540 signatures and 1,583 call sites, all
  mechanical. It cannot be split into hour-sized area submissions.

**A6. Collect-all behavior: holds.**
- The narrowed record leaves `diagnostics` as it is.
- No function gains an early exit.
- A failed check still leaves its findings in the shared list, in the same
  order.

**A7. Compile speed: neutral.**
- P4 measures no difference beyond noise.
- The gain is source size only: two parameters fewer on 540 signatures and two
  arguments fewer at 1,583 call sites.

## 4. The design that holds

```jet
// The parts one body check owns: made at its entry, gone at its end.
SemaBodyCheck: struct {
    state: SemaOwnershipState,
    tail_store: SemaStatementTailStore,
    context: SemaCheckContext,
}
```

- Checker signatures become `environment: SemaScopeEnvironment, program:
  SemaProgramTables, scopes: ^SemaScopeTable, check: ^SemaBodyCheck,
  diagnostics: ^[Diagnostic]`, which is 7 parameters down to 5.
- The 6 entries build the record from their locals and read
  `check.context.call_facts` and the like after the body, as they read the
  locals today.
- The switch is one mechanical change over the whole component. The tool that
  makes it is the sound way to land it; hand edits across 1,583 call sites are
  not.

The other parts of criterion E do not depend on this record and are
hour-sized: splitting `CheckProgram.jet` and `Statements.jet` by concern, and
replacing string-built lookup keys with typed keys.

## 5. The cutover as landed

The seven-part record of section 1 failed review (A1, A2, A5). Its redesign is
the three-part `SemaBodyCheck` of section 4, and that is what landed.

**Correction to P3.** P3 says Core has no swap or take primitive. That is
true of Core's library, but the language has the idiom without one: moving an
owned value out of a place and refilling that place in the same function
passes the ownership checker and copies nothing.

```jet
held: check.state
check.state = state
state = held
```

So a function holding a second ownership state or context can still call a
function that takes the record. It swaps its own part into the record, makes
the call, and swaps the part back. `Scopes.jet` names this idiom once:
`sema_body_check_swap_state`, `sema_body_check_swap_context` and
`sema_body_check_swap`. P3's conclusion about building a record from borrowed
parts stands. Swapping makes building one unnecessary.

**One pass.** A single codemod run made the switch over all of JetSema. No
caller outside JetSema takes the parts. The run's rules:

1. A function whose signature takes `state: ^SemaOwnershipState`,
   `tail_store: ^SemaStatementTailStore` and `context: ^SemaCheckContext`
   takes `check: ^SemaBodyCheck` in their place. It reads its parts as
   `check.state`, `check.tail_store` and `check.context`. Nine functions that
   took a state and a context are converted with the rest:
   `sema_expr_number_name_read`, `sema_expr_number_constant_read`,
   `sema_expr_number_constant_reread`, `sema_expr_number_operand_read`,
   `sema_expr_number_target_retype`, `sema_expr_check_identifier`,
   `sema_expr_check_shared_identifier`, `sema_expr_check_comptime_name` and
   `sema_string_view_materialize`. They call converted functions, and three
   of the number reads made a throwaway tail store for those calls.
2. The entries `sema_function_check_pass`, `sema_check_comptime_item_in` and
   `sema_marker_rules_check_static_sites` make the record
   (`sema_body_check_init()`) where they made the three locals.
3. A call that passes the record's own three parts passes `check`.
4. A call that passes the shared tail store with another ownership state or
   context is a check on that other state or context. The swap idiom wraps
   its statement.
5. A short table of named rows covers the sites whose shape the rules above
   do not cover. The run applies each row exactly once and refuses to write
   on a miss. The rows cover:
   - the record and its swap helpers;
   - five locals named `check` that would shadow the record;
   - the three throwaway tail stores of rule 1;
   - two swaps whose statement opens a block;
   - the default-argument check in another module, on the owner scope's state;
   - the default-value check's arena, traded rather than rebuilt;
   - the contract checks, which check pre and post clauses on a state of their
     own;
   - the eval probe, which reads its function from the record.
6. `jet fmt` then reshapes the 24 statements that the rename made too long or
   the dropped arguments made short enough to join. No file gains a formatter
   finding.

| Measure (JetSema) | Before | After |
|---|---|---|
| Functions taking `check: ^SemaBodyCheck` | 0 | 562 |
| Part parameters (`state`, `tail_store`, `context` borrows) | 2,153 | 495 |
| Parameters, all functions | 33,352 | 32,250 |
| Lines | 224,867 | 224,966 |
| Bytes | 13,479,887 | 13,423,036 |
| `.copy()` calls | 2,445 | 2,445 |

The run converted 556 signatures and renamed 7,156 part uses. It collapsed
1,643 calls, dropping 3,290 arguments, and wrapped 12 checks on another
state or context in swaps. The part parameters that remain belong to two
groups:

- 463 functions that take some of the parts but not all three: 243 take a
  state alone, 182 a context alone, 32 a state and a context, and 6 a tail
  store.
- The three swap helpers.

The switch leaves the `jet check` reports for JetSema, JetDriver and JetCLI
unchanged.

## 6. Helpers fed from the record

Once the record carried all seven parts, 1,086 JetSema functions still took
two or more parts as separate parameters. Most of them were called only
with the record's own parts (`check.scopes`, `check.program.*`), either
directly or through a caller that took those parts the same way. A census
of the call graph, iterated to a fixed point, found 775 such functions.
Each one now takes the record in the slot of its first part:
`check: ^SemaBodyCheck` when it writes a part, and `check: SemaBodyCheck`
when it only reads them. Each of the 2,220 calls passes `check` in place of
its part arguments.

The census leaves some functions out, and they keep their parameters:

- functions whose bodies already name `check`;
- functions used as values;
- functions with names defined more than once;
- functions where a call passes a part of another check, such as a probe's
  scopes, a default's owner scope or a lambda's state;
- functions called from outside a body check, such as registration or the
  graph passes;
- functions where a call writes through the record while another argument
  of the same call reads it.

| Measure (JetSema) | Before | After |
|---|---|---|
| Functions taking the record | 568 | 1,343 |
| Functions taking two or more parts without it | 1,086 | 432 |
| Part parameters | 3,867 | 1,970 |
| Parameters, all functions | 29,927 | 28,440 |
| Lines | 215,990 | 215,822 |
| Bytes | 13,065,120 | 12,956,387 |
| `.copy()` calls | 2,118 | 2,118 |

`jet fmt` reshapes the 223 statements whose length the change pushed over or
under the line limit. `jet check` on JetSema reports nothing new.
