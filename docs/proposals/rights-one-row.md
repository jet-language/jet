# Rights: one walker, one frame, ten codes

Status: proposal, 2026-09-01, revised after the review passes; element 1 of `whole-language-frame.md`. Independently adoptable. Ballots: D-RIGHTS1 (recommended C), D-RIGHTS-CLI1 (B), D-RIGHTS-DIAG1 (B). Nothing here is implemented; transcripts marked illustrative do not run today.

## Executive summary

"May this code do X here?" is one question. Jet answers it with the ratified rights row at four scopes (`-[FS.Read]>`, `#FX`, `authority.holds`, and twenty command-line flags), and then answers it again in a purity walker, a replayable walker, a comptime branch, and `--pure`. A separate transaction checker asks a different question, whether an irreversible effect inside `#Transact` has an undo, and must stay: `on_commit` and `#Undo` are exceptions a row by effect name cannot see (D-TXN1-4, D-BOUND-UNDO1).

The proposal keeps every ratified spelling and every ratified code. It deletes the three private walkers and the CLI copy after a differential matrix proves the verdicts identical. It gives every rights report one frame with the chain from the call to the refusing scope, and it replaces twenty flags with `--allow=` and `--deny=`, whose words are the manifest's own.

Score: mechanisms deleted 4 (purity walker, replayable walker, comptime branch, `--pure` flag) plus twenty flags; capabilities kept all; capabilities gained 3 (the chain in every denial, leaf-precise invocation caps, `@irreversible` as a declared fact the transaction checker reads).

| today | proposed | ballots |
|---|---|---|
| private walkers and CLI copy | one walker for rights rows; transaction checker stays | D-RIGHTS1 |
| twenty command-line flags | `--allow=` and `--deny=` | D-RIGHTS-CLI1 |
| moving one call changes the frame | one frame with the chain; codes stay | D-RIGHTS-DIAG1 |

## The problem: eleven coats

See `whole-language-frame.md`, Question 1, for the full table with homes. The short form:

| coat | spelling | private walker or table | fate |
|---|---|---|---|
| row | `-[FS.Read]>` | `Sema/Effects.rs:438-500` (canonical) | stays |
| purity | `-[]>` | `Sema/Purity.rs:77-99` | walker deleted; the row is empty |
| block | `#FX(Net)`, `#FX(grant: FS)`, `#FX(authority: IO)` | `effects_surface.rs:139-152` | stays (ratified D-AUTHORITY-SCOPE1) |
| package | `authority.holds` | `Authority.rs:239-326` | stays; own-code enforcement gap carded #2511 |
| module ceiling | manifest | `Sema/Effects.rs:232-280` | stays |
| replayable | `#Replayable` | `Sema/Effects.rs:1480-1565` | walker deleted; lowers to a row |
| transaction | `#Transact` | `Effects.rs:262-274` | checker stays; the table becomes a declared fact |
| comptime | `@ { }` | `crates/jet-comptime/src/Comptime/Purity.rs:78-123` | stage split and `impure_builtin` leaf table deleted; the block holds a row |
| pure eval | `--pure` | `Sema/Purity.rs:123-159` | flag stays as the spelling; lowers to the empty row |
| invocation | `--allow-net` … `--deny-gpu` | `Source/main.rs:1570-1774` | replaced by `--allow=`/`--deny=` |
| deny-only roots | `!Mem.Alloc`, `Panic` | `Sema/Effects.rs:66-132` | stay |

Two live probes show the cost. `jet eval --pure 'print("x")'` prints `x` and exits 0 (lane D probe 24): the CLI copy is weaker than the source walker. A package with `authority: { holds: { allow: [FS, Time] } }` whose own function declares `-[Net]>` and calls `http.get` passes `jet check` and `jet build` (verification block `rights`, #2511).

## The proposal

### One row at four scopes (ratified, unchanged)

```jet
use core.files as fs

fn load(p: String) String -[FS.Read]> { return fs.read(p) }     // callable row

fn run() {
    #FX(FS.Read) { print(load("notes.txt")) }                     // block narrows
    #FX(grant: Net, "vendor webhook") { print(fetch("https://x")) } // block loosens: one word, on the record
}
```

```jet
// package.jet
authority: { holds: { allow: [FS, Time] } }                       // package caps everything under it
```

### Presets lower to rows; the transaction checker stays (D-RIGHTS1 option C)

| spelling today | today | proposed |
|---|---|---|
| `-[]>` | separate purity walker | the row is empty; the one walker checks it |
| `#Replayable` | separate walker, fixed set | lowers to `-[!Time, !Rand, !Net, !IO]>` |
| `@ { }` comptime | policy branch | the block holds `-[Mem]>`; `#Impure("reason")` is the grant word (ratified D-ONCE-GATE1) |
| `jet eval --pure` | CLI copy | the spelling stays; it lowers to the empty row and the same walker refuses `print` |
| `#Transact(name) { }` | hand-written irreversibility table | the checker stays; `FS.Write`, `Net`, `Exec`, and `FFI` carry a declared `@irreversible` fact it reads (element 7) |

The spellings stay. What dies is the second implementation behind each row check. Precondition: a differential matrix runs purity, replayable, comptime, and invocation cases through the old walkers and the row walker and shows identical verdicts before any walker is deleted.

### One frame, the codes stay (D-RIGHTS-DIAG1 option B)

Today, moving one call changes the frame (real; codes from `tests/ui/effect_out_of_set.jet` and `tests/ui/effect_authority_out_of_set.jet`):

```jet
// package.jet
authority: { holds: { allow: [FS, Time] } }
```

```jet
// run.jet
use core.http as http
fn fetch(url: String) String -[FS]> { return http.get(url).body().text(1024) }   // E0740 today
fn run() {
    #FX(FS) { fetch("https://x") }                                                // E0712 today
}
```

Proposed frame (illustrative):

```text
Error [E0712]: this `#FX` region uses the effect `Net`, which it has no authority for
  run.jet:8:15   fetch("https://x")   reaches Net through fetch → core.http.get
  scope chain:   #FX(FS) at run.jet:8 → run → package orders (holds [FS, Time])
  nearest scope that could grant Net: package orders (package.jet:2)
  fix (needs-review):        add Net to authority.holds.allow in package.jet
  fix (behavior-preserving): move the call out of the #FX(FS) block
```

Every rights report reads the same way and keeps its code, and `jet check --json` carries a `denial_kind` field with the chain. The codes carry meaning a tool needs: a transaction denial (E0746) wants an undo, a pure-block denial (E3403) wants an injected `Clock`, a package denial (E1803) may need the call removed. E0750, an undeclared effect name, is a vocabulary error and keeps its own frame. A package grant is never a safe automatic edit: rights only widen by a written word (D-AUTHORITY-MODEL1), so that repair carries the ratified `needs-review` class and `jet fix` skips it by default.

### Invocation rights (D-RIGHTS-CLI1 option B)

| today | proposed |
|---|---|
| `jet run --allow-fs --deny-net --allow-time app.jet` | `jet run --allow=FS,Time --deny=Net app.jet` |
| cannot say allow `FS.Read` but deny `FS.Write` | `jet run --allow=FS --deny=FS.Write app.jet` |
| `jet eval --pure '1 + 2'` | unchanged spelling; lowers to the empty row |

Precedence: names expand to their leaves, then deny wins; the same effect in both flags is a usage error (E2102). Jet reads the two flags before the program's own flags; a program receives a literal `--allow` after `--`. The old twenty flags stop with a message that prints the exact replacement; scripts outside the repository are migrated by hand.

## Rungs

| rung | who | code | what the user types beyond the program |
|---|---|---|---|
| 0 | beginner | `fn save(t: String) { fs.write("out.txt", t) }` | nothing; `FS.Write` inferred (ratified D-EFFECT-OMIT1) |
| 1 | intermediate | `fn save(t: String) -[FS.Write]> { … }` | the row, as documentation the compiler holds you to |
| 2 | expert | `#FX(FS.Read) { … }` | a block that may do less |
| 3 | expert | `#FX(grant: Net, "vendor webhook") { … }` | a block that may do more, one word, on the ledger |
| 4 | expert | `authority: { holds: { allow: [FS, Time] } }` | the package cap |
| 5 | expert | `--allow=FS --deny=Net` | the invocation cap |

No upper rung changes rung 0. A beginner's inferred program is unchanged by any expert row above it, except that a cap can refuse it, which is the point of a cap.

## Three exits for the inference magic

| exit | today | proposed |
|---|---|---|
| see | `f.@effects` (ratified D-FACT-READ1); `jet inspect authority` | `jet inspect rights app.jet` lists every callable with its row and how it was inferred (illustrative) |
| write | `-[…]>` | unchanged |
| refuse | `authority: { holds: { allow: [] } }` | unchanged |

## Decisions

### D-RIGHTS1 — one rights walker

| option | what |
|---|---|
| A | one walker for everything, including transactions; rejected by the rival review because `on_commit` and `#Undo` are exceptions a row cannot express |
| B | keep every walker; unify only the words; drift stays possible, as `--pure` already shows |
| C (recommended) | one walker for rights rows at every scope; presets lower to rows; the transaction checker stays and reads a declared `@irreversible` fact; differential matrix before deletion |

Amends: D-REPLAY1 and D-SHAPE8 as lowerings. D-TXN1-4 and D-BOUND-UNDO1 unchanged. The command-line spelling is decided in D-RIGHTS-CLI1.

### D-RIGHTS-CLI1 — invocation rights spelling

| option | spelling |
|---|---|
| A | `--rights=+FS.Read,-Net` |
| B (recommended) | `--allow=FS.Read,Time --deny=Net`; deny wins after leaf expansion; duplicates are usage errors; read before the program's flags |
| C | keep twenty flags |
| D | `--allow FS.Read --allow Time --deny Net` (repeatable) |

Amends: D-CLI-GLOBAL1 flag set; reserves the two names against `#CLI` structs with a diagnostic.

### D-RIGHTS-DIAG1 — one frame

| option | what |
|---|---|
| A | one code for every denial; rejected by the rival review because tools lose the discriminator and E0750 is not a denial |
| B (recommended) | one frame with the chain and `denial_kind`; the ten codes stay; a grant is `needs-review`, never safe |
| C | keep the reports as they are |

Amends: the I4 snapshots of nine rows and their `jet.report/v1` detail fields; no code retires.

## What stays

| kept | why |
|---|---|
| `-[…]>` | callable row |
| `-[]>` | the row is empty |
| `#FX(...)` | block narrows |
| `#FX(grant:)` | one word, on the record |
| `authority.holds` | package caps everything under it |
| `#Unsafe("reason")` | one gate word |
| `#Impure("reason")` | the grant word |
| `#Nondeterministic("reason")` | one gate word |
| `#Replayable` and `#Transact` as spellings | the replayable walker lowers to a row; the transaction checker stays |
| `jet eval --pure` as a spelling | lowers to the empty row |
| the transaction checker | an irreversible effect owes an undo |
| all ten codes | the code stays as the discriminator |
| the ambient default for a file with no manifest (D-AUTH-AMBIENT1) | ratified |
| `Mem` and `Panic` as deny-only roots | rights a scope can give up |

## Implementation shape

| phase | work |
|---|---|
| A | the differential matrix; `@irreversible` declared in `Prelude/Effects.jet` and read by the transaction checker; one walker consumes rows from every scope kind; `Sema/Purity.rs:77-99`, the replayable walker, and the comptime stage split in `crates/jet-comptime/src/Comptime/Purity.rs:78-123` deleted after the matrix is green |
| B | none owed; #2511 (own-code enforcement of `authority.holds`) is a separate defect card |
| C | after D-RIGHTS-DIAG1: the frame with the chain and `denial_kind`, snapshots re-blessed; after D-RIGHTS-CLI1: `--allow=`/`--deny=`, the twenty flags removed from `CLI.rs` and `main.rs` with a replacement message |

## Where Jet loses today

| peer | what it does better | Jet today |
|---|---|---|
| Koka | one effect abstraction with inference and handlers | four copies of the checker and twenty flags |
| Deno | one permission grammar on the command line | four copies of the checker and twenty flags |
| WASI | one permission grammar on the command line | four copies of the checker and twenty flags |

## Strongest unverified assumption

That every private walker is semantically a row check. Lane C read the source of each; the rival review already found one that is not (the transaction rule), and the design changed. The differential matrix in phase A is the proof for the rest.

| assumption | how it is proved | where |
|---|---|---|
| every private walker is semantically a row check | the differential matrix | phase A; card #2501 |
