# The verdict loop: repair coverage by ratchet, one status object

Status: proposal, 2026-09-01, revised after the review passes; element 5 of `whole-language-frame.md`. Independently adoptable. Ballots: D-VERDICT-LOOP1 (recommended D), D-CLI-ONE1 (A). Nothing here is implemented; transcripts marked illustrative do not run today.

## Executive summary

Every Jet report is already one `jet.report/v2` JSON object per line with a registered code, what, why, fix, location, span, and `fix_edits` carrying one of five safety classes (D-REPORT-MACHINE1, D-REPORT-FIXGRADE1=D). `jet fix` applies the `formatting` and `behavior-preserving` classes by default. That contract is right. Three things are missing around it: coverage (83 of 762 active rows carry `fix_edits`), one machine status object on pass and on fail (today a pass prints a bespoke `{"status":"passed",…}` and a fail prints report lines), and a rule that keeps the hole from growing.

The proposal keeps the ratified report line and safety classes untouched. It adds a typed `no_fix_reason` field, mutually exclusive with `fix_edits`, holding a reviewed reason and a next action. It adds a per-plane coverage baseline that never decreases: a new row without either fails the registry today, and existing rows are classified plane by plane. Structured-edit coverage is reported apart from reason coverage so a reason cannot masquerade as a repair. Machine status becomes one `jet.status/v1` object for every command in `--json` mode, wrapping the ratified report lines; human progress and usage lines stay plain, as the report law requires. The tool tree gets one `jet inspect <plane>` per ledger plane at four scopes, task actions keep their names, and the registry generates the parser under every option.

Score: mechanisms deleted 3 (bespoke pass status, hand parser as a second authority, six duplicate ledger views); capabilities kept all; capabilities gained 3 (a reason on every uncovered row, a nondecreasing coverage floor, one view per plane at four scopes).

| today | proposed | ballots |
|---|---|---|
| 83 of 762 rows carry a repair; two status shapes | per-plane ratchet; one status object | D-VERDICT-LOOP1 |
| 58 commands; six ledger views under other names | one view per plane; task actions keep their names | D-CLI-ONE1 |

## The problem

| defect | evidence |
|---|---|
| repairs are rare | 901 rows, 762 active, 83 with `fix_edits` (`Prelude/Diagnostics.jet`; `Registry.rs:1009-1017`) |
| two status shapes | pass `{"status":"passed","schema_version":1…}`, fail report lines (`Source/CmdInspect.rs:959-1038`; lane D probes 10, 30) |
| fix output is prose | `jet fix --json hello.jet` → `hello.jet: no changes made` (probe 28) |
| infrastructure failures drop `--json` | `test --json` and `prove --json` print plain E2105 text (probes 20, 21) |
| outer success hides inner failure | `inspect compiler check bad.jet` exits 0 with `status: ok` around E0003 (probe 29) |
| two CLI authorities | `Source/main.rs:1570-1774` and `crates/jet-cli/src/CLI.rs:1404-1575` both know every flag |
| smuggled payloads | `Report.with_fields` accepts pre-encoded JSON text (`Report.rs:297-310`); fix metadata rides `source_edit|safety` strings (`Diagnostics.jet:3-5`) |
| the tree | 58 commands; `inspect` has 32 actions, six of which are ledger views under other names |

Lane E's recurring-complaint table lists "diagnostics readable without being action complete" in five audits and "machine report/LSP parity partial" in four.

## The proposal on the page

### One status object around the ratified report line (D-VERDICT-LOOP1 option D)

The two files:

```jet
// package.jet, line 2
authority: { holds: { allow: [FS, Time] } }
```

```jet
// run.jet
use core.http as http
fn fetch(url: String) String -[FS]> { return http.get(url).body().text(1024) }
fn run() {
    #FX(FS) { fetch("https://x") }      // line 8, column 15 is the call
}
```

Proposed output (illustrative):

D-REPORT-MACHINE1 already says that command status envelopes remain status data and that every report inside them uses the report object. `jet.status/v1` is that envelope, made one shape for every command; each report inside it is the ratified object, unchanged except for one optional field.

```text
$ jet check --json run.jet
{"schema":"jet.status/v1","action":"check","ok":false,"reports":[
 {"schema":"jet.report/v2","moment":"compile","severity":"error","code":"E0712",
  "what":"this `#FX` region uses the effect `Net`, which it has no authority for",
  "why":"…","fix":"…","location":"run.jet:8:15","span":"8:15-8:32",
  "fix_edits":[{"file":"package.jet","span":"2:31-2:31","new_text":", Net","safety":"needs-review"}]}
]}
$ jet fix --json run.jet
{"schema":"jet.status/v1","action":"fix","ok":true,"applied":0,"skipped":[{"code":"E0712","safety":"needs-review"}]}
$ jet check --json ok.jet
{"schema":"jet.status/v1","action":"check","ok":true,"reports":[]}
```

The report object is exactly the ratified line; `span` names the two characters after `[FS, Time` inside the list, so the edit inserts `, Net` before `]`. A row that has no edit carries a reason instead:

```text
 {"schema":"jet.report/v2","code":"E0631","what":"`x` cannot be shared — it does not live long enough to be returned",
  "no_fix_reason":{"kind":"design","next":"return an owned value or widen the region; see jet://explain/E0631"}}
```

`no_fix_reason.kind` is one of `behavior`, `design`, `ambiguous`; `next` is a reviewed action or link. The registry guard fails a row that has neither `fix_edits` nor `no_fix_reason`, exactly as it already fails a fact row without a safe direction (D-FACT-LAW1=B).

### The ratchet

| plane | rows | with `fix_edits` | with a reason | uncovered | state |
|---|---|---|---|---|---|
| rights | 52 | 41 | 11 | 0 | closed |
| types | 301 | 120 | 60 | 121 | open |

`jet inspect build --coverage` prints that table (illustrative). The baseline per plane never decreases; a new row in an open plane without an edit or a reason fails the registry today; a plane closes when its uncovered count is zero. The two coverage numbers are reported apart, so classifying the 679 uncovered rows as `design` in bulk would show as reason coverage, not repair coverage.

### The loop

```text
emit      $ jet check --json run.jet       → ok: false, one report, one needs-review edit
edit      $ jet fix run.jet                → applies formatting and behavior-preserving edits; lists the skipped one
decide    edit package.jet by hand, or move the call
re-run    $ jet check --json run.jet       → ok: true, reports: []
```

The agent never chooses between fixes the compiler did not name, and never applies a grant the law reserves for a written word.

### The tool tree (D-CLI-ONE1 option A)

`jet inspect <plane> [TARGET] [--live PID | --replay ARTIFACT]`, planes `types`, `rights`, `claims`, `shapes`, `structure`, `build`, `gates`. `TARGET` defaults to the current package; `--live` and `--replay` are exclusive.

| today | proposed |
|---|---|
| `inspect facts` | `inspect types` |
| `inspect authority`, `inspect guarantees` | `inspect rights` |
| `inspect unsafe` | `inspect gates` |
| `inspect structure`, `inspect gates` | unchanged names, ledger-backed |
| `report`, `dossier` | `inspect build` |
| `inspect live <pid>` | `--live PID` on every plane |
| `inspect schema`, `codemod`, `sbom`, `bind`, `logs`, `info`, `outdated`, `digest`, `env`, `expand`, `output`, `provenance`, `semindex`, `impact`, `graph`, `query build`, `explain-build`, `compiler`, `audit` | unchanged: task actions, not ledger views |
| `jet explain <code or symbol>` | unchanged |

Six names retire (`facts`, `authority`, `unsafe`, `guarantees`, `report`, `dossier`); `jet help` prints each old name with its new home for one edition. The registry generates the parser, and a drift guard fails when `main.rs` knows a flag the registry does not.

```text
$ jet inspect rights run.jet
load   -[FS]>   declared; package holds FS
run    -[FS]>   inferred via load
$ jet inspect claims --live 4242
add_fee.Post   generated(1000)   also: examples(1)
$ jet inspect build --replay .jet/replays/c10aee50.jetproof-replay
identity 9f3a…   engine dev-tir-v1   14 clock events   outcome R0802
```

## Rungs

| rung | who | what they see |
|---|---|---|
| 0 | beginner | the terminal error with the chain and the fix line; `jet fix` applies the safe ones |
| 1 | intermediate | `jet inspect rights` or `claims` to see why |
| 2 | expert | `--json` everywhere with the same status object; LSP code actions from the same rows |
| 3 | expert, agent | `jet check --json` → `jet fix` → `jet check --json`; the loop terminates when `reports` is empty |

## Three exits

| exit | spelling |
|---|---|
| see | `jet explain <code>` prints the row: what, why, fix, `fix_edits` with class, or `no_fix_reason` |
| write | `jet fix --all` applies every class (ratified); `jet fix --dry-run` shows the diff |
| refuse | `#[allow(lint)]` per site and `policy.lints` per package stay the words (ratified D-LINTPOLICY1) |

## Decisions

### D-VERDICT-LOOP1 — coverage and status

| option | what |
|---|---|
| A | a global guard now; rejected by the rival review: it blocks the registry on 679 rows and invites bulk reasons |
| B | one status object; coverage as ongoing work |
| C | guard errors only; lints exempt forever |
| D (recommended) | per-plane ratchet with a reviewed reason field; one `jet.status/v1` object; five safety classes unchanged; the report object gains one optional field under `jet.report/v2`; edit coverage reported apart from reason coverage |

Amends: D-DX1 (status envelope); the I4 registry guard (baseline rule); D-REPORT-MACHINE1 additively (one optional typed field `no_fix_reason` on the report object under schema `jet.report/v2`; the one-object-per-line law and its envelope clause unchanged). D-REPORT-FIXGRADE1 unchanged; status, progress, success, and usage lines stay plain as D-REPORT-LAW1 requires.

### D-CLI-ONE1 — the tool tree

| option | what |
|---|---|
| A (recommended) | one view per plane, four scopes; six ledger-view names retire; task actions keep their names; parser generated |
| B | keep the tree; add `jet inspect ledger` |
| C | a new top-level word `jet ledger <plane>` |

Amends: D-DX4, D-SHAPE-CLI-COMPLETE1 (command inventory).

## What stays

| kept | why |
|---|---|
| `jet check` | command status |
| `jet fix` | applies the safe ones |
| `jet explain` | prints the row |
| the report object as one object per line (`jet.report/v2` adds one optional field) | ratified report line |
| the five safety classes | ratified |
| `jet fix --all` | applies every class |
| LSP quick fixes from the same rows | same rows |
| `#[allow(lint)]` | per site |
| `policy.lints` | per package |
| every task action under `inspect` | task actions keep their names |
| every existing code | code stays |

## Implementation shape

| phase | work |
|---|---|
| A | `jet.status/v1` on every command's `--json` path including infrastructure failures; `Report.with_fields` takes typed fields; the parser table generated from `CLI.rs` with a drift guard |
| B | none owed |
| C | after D-VERDICT-LOOP1: the `no_fix_reason` field, the per-plane baseline and guard, `jet inspect build --coverage`; after D-CLI-ONE1: the plane views, the folds, the help map |

## Where Jet loses today

| peer | what it does better | Jet today |
|---|---|---|
| Rust | `cargo fix` and clippy apply machine edits across most lints, and coverage grew lint by lint | the row model and one row in nine |
| TypeScript | its language service is one authority for every client | the row model and one row in nine |

## Strongest unverified assumption

That 679 rows can each state an edit or an honest reviewed reason. The audit counted rows; it did not classify them. The ratchet makes the count visible per plane, and the card reports the split between edits, `behavior`, `design`, and `ambiguous`.

| assumption | how it is proved | where |
|---|---|---|
| 679 rows can each state an edit or an honest reviewed reason | the per-plane ratchet and the split between edits, `behavior`, `design`, and `ambiguous` | card #2505 |
