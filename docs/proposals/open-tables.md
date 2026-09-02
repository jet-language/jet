# Open tables: Core is Jet, declared in one place, bootstrapped in one order

Status: proposal, 2026-09-01, revised after the review passes; element 7 of `whole-language-frame.md`. Independently adoptable; phase A is internal. Ballot: D-OPENTABLE1 (recommended D). Nothing here is implemented.

## Executive summary

The corpus law says every truth has one home and every other copy is rendered from it (D-ONCE-LAW1=A). Inside the compiler, five tables break it. Effect roots, the Core module list, the Core call dispatcher, the keyword list, and the sema derive registry are hand-written Rust tables that repeat facts already declared in Jet source, or that should be. Lane F found 622 `CoreCallRecord::new` rows and 145 ambient routes with no generator, `core.services` versus `core.service` drift, and an alias table that survives only because the dispatcher and the ledger disagree.

The proposal splits the tables by whether a Jet twin exists. Tables with a twin (effect roots and leaves, marker sites, fact reads, encoding formats, taint sinks, transaction irreversibility, the Core dispatcher, ambient routes, the export classifier) are generated from the declaration with a drift guard that fails the build on a hand row. Tables with no twin (the lexer keyword map, the CLI inventory, the report enums, the tier list) keep their Rust home as the one home under a guard that fails when a second copy appears; the parser must read the keyword table before any Prelude file exists, so generating it is a bootstrap cycle. Generation runs in an order that resolves before it declares: roots and reservations first, then modules and routes, then derives. Nothing in user syntax changes, and user packages cannot add Core roots; the phrase "closed to everyone" in the first draft meant closed to users, and this draft says so.

Score: mechanisms deleted 4 (four hand tables with twins become generated); capabilities kept all; capabilities gained 1 (a drift guard on every table, generated or not).

| today | proposed | ballots |
|---|---|---|
| five hand tables repeat Jet declarations | generate where a twin exists; guard the rest | D-OPENTABLE1 |

## The problem

| table | home | declared where | defect |
|---|---|---|---|
| effect roots | `crates/jet-foundation/src/Effects.rs:11-25`, `Authority.rs:27-45` | `Prelude/Effects.jet:1-20` (declarations) | two Rust enums repeat the Jet declarations; `FFI` is irreversible in one (`Effects.rs:266-274`) and absent in the other |
| Core modules | `CoreModuleExports.rs:20-135` | nowhere in Jet | hand-coded export kinds; `core.services` vs `core.service` |
| Core calls | `CheckerCoreLib/core_call.rs:420-449,450-596,956-960` | nowhere | 622 rows plus 145 ambient routes hand-written; the alias table `core.task -> core.tasks` |
| keywords | `crates/jet-lexer/src/Lexer/mod.rs:46-84` | `Syntax.rs` (832 lines, ratified) | the lexer repeats the Syntax table; `Syntax::KW_SWITCH` is literally `"if"` and `Stmt::Switch` has no live grammar (lane A, card #2512) |
| derives | `Sema/Registration/Derives.rs:7-80` | nowhere | user templates expand into generated items that still depend on this closed table |

Lane F's silhouette lists 34 documented Core domains with no dispatcher rows, including `compute`, `plugin`, `reactive`, `web`, and `storage`, and two `core.services` rows the ledger does not name.

## The proposal

### One declaration, generated tables

```jet
// crates/jet-codegen/src/Prelude/Effects.jet (today, ratified: declarations exist)
effect FS { Read, Write }
effect Net
effect Time
// proposed additions: the facts the Rust tables currently carry by hand
effect FS.Write @irreversible
effect Net @irreversible
effect Exec @irreversible
effect FFI @irreversible
```

`@irreversible` is a declared fact on the effect leaf; the transaction checker (element 1) reads it instead of its private table. Today the checker's table already treats FFI as irreversible; the declaration makes that fact visible and single-homed.

```jet
// proposed: crates/jet-codegen/src/Prelude/Core.jet
module core.files exports { read, write, exists }      // one row per Core module, with its export kinds
module core.tasks exports { spawn, join, all }        // the ledger's spelling; the core.task alias and dispatcher row die
```

From those declarations the build generates `Effects.rs` enums, `CoreModuleExports.rs`, the dispatcher rows, and the ledger `core-surface-ledger.json`. A generator diff is a build failure, exactly like the existing `Syntax.rs` drift checks (`tests/syntax_ledger.rs`).

### Bootstrap order

| step | what | why first |
|---|---|---|
| 1 | `Syntax.rs` (ratified, hand-maintained, the one home for keywords and sigils) | the lexer needs it before any Jet file is read |
| 2 | effect roots and lexical reservations declared in `Effects.jet`; a generated `Effects.rs` checked in and diffed | sema needs roots before the Prelude modules are checked |
| 3 | `Core.jet` module and route declarations; generated dispatcher and ledger | routes need roots and types |
| 4 | derives: the registry becomes a declaration in the Prelude with the same template mechanism user code uses (D-ONCE-DERIVE1) | last, because it depends on everything above |

The generated Rust is checked in so the compiler builds without running itself; the diff guard proves it matches the declarations.

## Rungs

| rung | who | what changes |
|---|---|---|
| 0 | beginner | nothing; `use core.files as fs` works as before |
| 1 | user package | nothing new; packages declare their own effects as ratified (`effect Payments`, D-EFF4); Core roots stay closed to users |
| 2 | compiler author | one declaration per Core fact; the tables are generated; the guard names the drift |

## Three exits

| exit | spelling |
|---|---|
| see | `jet inspect structure --core` lists every Core module and root with its declaration site (illustrative) |
| write | the declaration in the Prelude |
| refuse | not applicable; this is compiler-internal |

## Decision

### D-OPENTABLE1 — how Core facts are homed

| option | what |
|---|---|
| A | Generate every table: every table listed in the proposal is generated from a Jet declaration. Rejected by the rival review: the lexer keyword map, the CLI inventory, and the report enums have no declared twin, and a compiler needs those to parse the declarations that would generate them. |
| B | Generate the dispatcher and the effect enum only: the two largest tables are generated; the small vocabularies stay hand-written. Two generators, no guard on the rest. |
| C | Fix `core.services` by hand: no generator, no change. |
| D (recommended) | Generate every table that has a declared twin; guard the rest as their own home. Tables with a Jet twin are generated from it with a drift guard that fails the build on a hand row: effect roots and leaves, marker sites, fact reads, encoding formats, taint sinks, transaction irreversibility, the Core dispatcher, ambient routes, and the export classifier. Tables with no twin keep their Rust home as the one home under a guard that fails when a second copy appears: the lexer keyword map, the CLI inventory, the report enums, the tier list. Irreversibility is declared on `FS.Write`, `Net`, `Exec`, and `FFI`. Precondition: a derivability census of the 622 dispatcher rows lists every row the generator cannot derive; each becomes a declared fact or a card before the dispatcher switches. |

Amends: D-META-ONE1 scope note (Prelude-only markers unchanged; the Rust twin retires) and D-META-REG1 (the registry as generator input). D-ONCE-LAW1 requires one home and a guard, not a Jet home, and D respects that. `core-surface-ledger.json` stays the rendered view (D-CORE-TREE1 unchanged); D-CORE-CALL1's floor (user code cannot call `__core_intrinsic`) is unchanged. Generation is a workspace build step over committed declarations, checked in like the embedded Prelude, so there is no bootstrap cycle: the lexer that parses the declarations is not generated from them.

## What stays

| kept | why |
|---|---|
| `Syntax.rs` as the ratified keyword and sigil home | one home for keywords and sigils |
| user-declared effects | packages declare their own effects |
| `__core_intrinsic` as compiler-only | user code cannot call it |
| every Core module name that the ledger already lists | the ledger lists the names |
| `core-surface-ledger.json` as the rendered view | rendered view |

## Implementation shape

| phase | work |
|---|---|
| A | steps 1-3: declarations, generators, checked-in output, diff guard; the `core.task` alias row and dispatcher row deleted; `core.tasks` is the one spelling; #2513 (`core.services` and `core.task` drift) closes as part of this |
| B | step 4: derives |
| C | after D-OPENTABLE1: nothing further owed; the ballot chooses the order |

## Where Jet loses today

| peer | what it does better | Jet today |
|---|---|---|
| Rust | `core` and `std` are one crate tree the compiler reads | Core is declared in Jet and repeated in Rust five times |
| Zig | `std` is Zig source with `comptime` reflection and no parallel table | Core is declared in Jet and repeated in Rust five times |

## Strongest unverified assumption

That every dispatcher row can be derived from a declaration. Lane F sampled rows and found hand-written argument adapters in some (`core_call.rs:450-596`); those adapters stay as marshalling code, and only the row that names them is generated. The phase A generator diff is the proof.

| assumption | how it is proved | where |
|---|---|---|
| every dispatcher row can be derived from a declaration | the phase A generator diff | card #2507 |
