# D-AUTH-DEFAULT1 adversarial design review (2026-11-08)

Card #6614 c1. The owner ratified D-AUTH-DEFAULT1=A: every effect is allowed
by default, code denies, and a run warns when its effect set changes. This
review attacks each strength the ballot claims against Deno (deny by default
plus prompts), Node (opt-in `--permission`), and Python, Go and Rust (no
authority model), with probes run on the current compiler. It ends with a
verdict; every gap goes to a card, a criterion or a ballot.

## Measured facts

Every probe ran the seed compiler `jet@0.0.1#4a7b744c` with
`JET_TOOLCHAIN_ROOT` set to a `git archive` of origin/dev `b53d287e41`,
`NO_COLOR=1`, and a fresh `JET_STORE_DIR` under
`$JET_SCRATCH_ROOT/lanes/EffectsLead/probes`. Outputs are quoted verbatim.

| # | Probe | Observed |
|---|---|---|
| P1 | file with no `package.jet` calling `files.read` | `jet run` stops with E1803 "Application authority is undecided for `FS.Read`", exit 1. `-A` runs. `--verbose -A` prints `jet run: effects used: FS.Read, IO`. `-A --deny=FS` stops with E1803 "denies `FS.Read`". |
| P3 | the same body behind a `#!` launch line | runs with no flag: a script answers as `-A` does (cli.md "Application authority") |
| P4 | `authority: { deny: [Net] }` plus `tcp_listen` | E0749 with the call chain at `jet check` and `jet run`, before anything runs |
| P6a | a pure function stored in a list and called | runs; the run's effect list is `IO` |
| P6b | the P6c function returned through `pick(table)` | **SIGSEGV** in `pick.fn_widen@308`, exit 139 (codegen defect, unrelated to authority) |
| P6c | `peek` reads a file; `run` stores it in a list and calls the element (source below) | runs with **no** flag and reads the file; `--verbose -A` prints `effects used: IO`. The run's effect list misses `FS.Read`, and the E1803 gate does not fire. |
| P6d | P6c with `authority: { deny: [FS] }` and `run: fn() -[IO]> Unit` | E0740 names the maximal row at the element call, E0749 reports `peek` and `run`: sema's per-function row is sound |
| P6e | a file-reading function passed as a parameter | `effects used: FS.Read, IO`; plain `jet run` stops with E1803 |
| P8a | `#Import c.c: module { getpid }` | runs with no flag; effect list `IO` |
| P8b | `deny: [FFI]`, `run: fn() -[IO]> Unit` calling C `getpid` | `jet check` ok, `jet run` runs |
| P8c | `deny: [FFI, Exec, FS]`, `-[IO]>`, C `system("touch escaped.txt")` (source below) | prints `system=0`, exit 0, and `escaped.txt` exists. The deny row, the `-[IO]>` bound and the floor are all bypassed. |
| P9 | `comptime { files.read(...) }` | E3410: a comptime effect needs `#Impure` and `--gate impure=allow`, with or without `-A` |
| P12 | `deny: [Net]` package that starts `sh` | runs: `Exec` is not bounded by the rights of the program that starts it |

P6c:

```jet
use files: core.files

peek: fn(x: Int) -> Int {
    text: files.read("hidden2.jet") ?? ""
    text.len() + x
}

run: fn() {
    table: [fn(Int) -> Int]{peek}
    f: table[0]
    print("v={f(0)}")
}
```

P8c (`package.jet` holds `authority: { deny: [FFI, Exec, FS] }`):

```jet
use c: c.c
#Import c.c: module {
    system: fn(command: String) -> I32 = "system"
}

run: fn() -[IO]> Unit {
    code: c.system("touch escaped.txt")
    print("system={code}")
}
```

### Roots in the source

- **P6c.** `jet_driver_required_effects`
  (`Compiler/JetDriver/Source/Driver/Pipeline.jet`) walks call edges from the
  entry and collects `effects.direct`. Its comment states the choice: "the
  maximal row an open-world call ... seeds into `solved` is a ceiling fact
  (E0743), not a required right, so it never widens the authority the
  application must hold." A function named as a value is no call edge, so
  its row never reaches the run's list. Under deny by default that choice
  traded soundness for fewer refusals; P6c shows it also let a file read
  pass the E1803 gate.
- **P8.** The parser builds every written `#Import c.<lib>: module` function
  with `effect_root: None` (`JetParser/.../Declarations/Foreign.jet`), and
  `sema_registration_callable_abi_for_extern` copies that `None` into the
  callable ABI. `sema_effect_record_fact` then sees a callee with a key, no
  body and a known (empty) signature row, so it adds nothing: "A callee with
  no body in this program has no effects". Inline `#FFI(c)` bodies get
  `FFI.C` (`sema_registration_inline_foreign_effect_root`), generated
  bindings carry their descriptor root, and written C imports carry nothing.
  spec.md "Effects" says an opaque C or extern call contributes the maximal
  set, which does not hold today. The `#Unsafe` gate is opt-in: safety.md
  covers a `#Import(c)` declaration only when it is gated, so for an ungated
  binding the effect row is the only bound, and that row is empty.
- **Prompt path.** cli.md "Application authority" records that the
  interactive `Run it? [Y/n/always]` confirmation was never ported: "the Jet
  host always answers as a non-interactive run." The prompt case is
  known-failing (#5454). What exists is the E1803 undecided refusal, its
  inline-block Fix, `jet fix`'s inline grant edit, and the script `-A` rule.
- **Remote code.** No command runs code straight from a URL or a registry
  reference. `jet X` resolves a command, a job, a source path, then `jet-X`
  on `PATH` (D-JET-VERBS1, syntax-decisions.md); dependencies arrive only
  through jetpack, which records per-package `effects`, `required_effects`,
  `granted_effects` and `denied_effects` in the lock today
  (`Jetpack/PackageModel/Source/Lock/Schema.jet`).

## Peer practice

- **Deno**, deny by default
  (https://docs.deno.com/runtime/fundamentals/security/). In a terminal an
  unlisted access prompts `Allow? [y/n/A]`; without a terminal it throws
  `NotCapable`. `--deny-*` overrides `--allow-*`. The docs state that
  `--allow-run` and `--allow-ffi` are equivalent to allow-all, that `-A`
  "turns the sandbox off entirely", that all code on a thread shares one
  privilege level, and that static imports load with no permission check.
- **Node**, allow by default with an opt-in model
  (https://nodejs.org/api/permissions.html). `--permission` has been stable
  since v23.5.0 and v22.13.0. Node calls it a "seat belt" that does not
  protect against malicious code. `--permission-audit` reports and does not
  deny. The `fs` permission does not cover `node:sqlite`, and `--allow-addons`
  and `--allow-child-process` are escape hatches.
- **Python** has no authority model. PEP 578 audit hooks
  (https://peps.python.org/pep-0578/) observe events and can raise, but the
  PEP says they are not a sandbox.
- **Go** has no authority model. google/capslock
  (https://github.com/google/capslock) statically maps each package to
  capabilities (`FILES`, `NETWORK`, `EXEC`, `CGO`, `UNANALYZED`, ...) and
  diffs them across versions. It marks cgo and reflection as their own
  capabilities and does not claim to see through them.
- **Rust** has no authority model. cargo-cackle
  (https://github.com/cackle-rs/cackle) enforces per-crate API allow-lists and
  sandboxes build scripts. cargo-vet and cargo-crev record human audits.
  `unsafe` and FFI are outside what any of them can bound.
- **Java** removed the SecurityManager (JEP 411 deprecation, JEP 486 removal
  in JDK 24, https://openjdk.org/jeps/486). Its stated reasons were that the
  fine-grained runtime permission model was rarely used, was costly to
  maintain, and imposed run-time cost. That is the strongest evidence against
  deny by default with runtime checks.
- **npm** `npx` asks "Ok to proceed?" before installing a package it is
  about to run (https://docs.npmjs.com/cli/commands/npx). That is the only
  first-run question in the set.

## Claims, attacks and verdicts

### C1. No beginner friction: a first program runs with no flag or manifest

**Attack.** Node, Python, Go and Rust already have zero friction, so this is
parity, not a strength. Deno's prompt costs one keypress per right, and its
`-A` is one flag.

**Evidence.** P1 is refused today. Under D-AUTH-DEFAULT1 it runs and prints
one notice line. P9 shows the one place a beginner still meets a gate, and
it is a gate every comparator lacks: effects while compiling.

**Verdict: holds.** It is parity with Node, Python, Go and Rust, plus a notice
none of them gives. Against Deno it saves a prompt or a flag per program.

### C2. Nothing changes silently: the notice names every effect the run can reach

**Attack.** The notice is only as true as the row it prints. Capslock reports
`UNANALYZED` and `CGO` instead of guessing. A notice that omits an effect is
worse than none, because it reassures.

**Evidence.** P6c reads a file, and its run list says `IO`. P8a and P8c call
C, and their list says `IO`. Both come from the projection in
`jet_driver_required_effects`, which drops the maximal row, and from written
C imports carrying no effect.

**Verdict: fails as built.** It holds after redesign R1 and R2 below. With
them, an open-world call shows up in the notice as "can do anything", the
same honesty capslock's `UNANALYZED` and Deno's "`--allow-ffi` is allow-all"
give.

### C3. A written ceiling or deny is enforced when checking, before anything runs

**Attack.** Deno and Node enforce at the syscall boundary, so native code
loaded with `--allow-ffi` or `--allow-addons` is outside their model too. The
check-time claim is only better if nothing escapes the checker.

**Evidence.** P4 and P6d hold: E0749 with the chain at `jet check`, including
through a function value. P8b and P8c fail: a written C import escapes `deny`
and the `-[IO]>` bound, and it wrote a file under `deny: [FS]`.

**Verdict: fails for C imports, holds otherwise.** It holds after R2.

### C4. Static ceilings cost nothing at run time

**Attack.** Static analysis can't see through processes the program starts.
Deno documents that `--allow-run` amounts to allow-all, and Java dropped its
runtime checks for cost.

**Evidence.** P12: a `deny: [Net]` program that starts `sh` gets a shell with
network access. The ceilings are erased before code generation (D-EFF1), so
there is no runtime check to pay for. The one runtime mechanism,
`--allow=FS.Read:<path>`, is a kernel confinement (Landlock), opted into per
run.

**Verdict: holds within limits.** `Exec` and `FFI` are escape hatches in Jet as
in Deno. The notice must say so (R4) rather than imply a child process
inherits the program's rights.

### C5. Dependency widening is reported, and `--locked` refuses it

**Attack.** Capslock and cackle already diff capabilities across versions for
Go and Rust. Jet's advantage holds only if the rows are sound, because a
widening hidden behind a function value or a C import is the supply-chain
attack itself.

**Evidence.** The lock already stores per-package effect rows (Measured facts,
"Remote code"). The rows come from the same inference P6c and P8 defeat.
Granularity is the effect root or leaf, not the resource: `FS.Read` does not
say which files.

**Verdict: holds within limits after R1 and R2.** The limit is granularity. A
resource-qualified ceiling (`FS.Read:<path>`) is the existing tool for teams
that need more.

### C6. Code from elsewhere asks before its first run

**Attack.** Deno's static imports run with no check, and `git clone` then
`jet run` is "code from elsewhere" that this rule never sees.

**Evidence.** No command runs a URL or registry reference (Measured facts). A
dependency enters through jetpack, whose widening report (C5) runs before any
code does. A cloned repository is local code.

**Verdict: holds within limits.** Today the rule has no subject. #6614 c8
needs a run-from-URL or run-from-registry command, which is new surface (gap
G3). A clone is local code, and that is the named loss in C8.

### C7. "Prompts train users to press yes, so most users pass `-A`"

**Attack.** This is the ballot's evidence for dropping the prompt. Nothing in
the repository measures it.

**Evidence.** Deno's docs warn about `-A` and about `--allow-run` and
`--allow-ffi`. Java's JEP 486 records low use of fine-grained permissions.
Neither measures "most users".

**Verdict: fails as stated.** It holds once narrowed to what the sources say:
coarse grants are documented as defeating the model, and Java removed its
fine-grained model for lack of use (gap G7).

### C8 (named loss). Local code runs with full authority

**Attack.** Deno users get a prompt on a cloned project's first file read, and
Jet users get one notice line.

**Evidence.** Python, Go, Rust and Node (without `--permission`) behave the
same as Jet, with no notice at all. The notice repeats whenever the set
changes, and a written ceiling turns any widening into a check error (P4).

**Verdict: holds within limits.** It is the accepted cost of the ruling. The
mitigation is the notice and the ceiling, not a prompt.

## Redesign

These are build preconditions for #6614 c3 onwards, so the notice never ships
on an unsound row.

- **R1. The run's effect list is sema's sound row.** When the entry reaches
  an open-world call (an unannotated function value, or an extern with no
  root and no declared row), the list carries the maximal set. The notice
  says the program "can do anything" and names the first such call
  (`file:line`), the way E0740 already does. A named function used as a
  value also adds its precise row as a reference edge, so a list of known
  functions (P6c) reports `FS.Read` without widening to everything.
- **R2. A written C import is a foreign call.** It records the `FFI.C` leaf,
  and with no declared effect row it is open-world (maximal), as spec.md
  already says. `deny: [FFI]`, a `-[IO]>` bound and a `deny` of any root then
  refuse P8b and P8c at check time with E0749 or E0740. A gated
  `#Unsafe` binding keeps its existing audited-region rule (E3103).
- **R3. One rule for scripts.** The `#!` launch line stops answering as `-A`.
  A script gets the same notice as any program, which is what the ruling
  says ("when a script or program is first run it warns ... unless using
  the `-A` flag").
- **R4. Notice wording names the escape hatches.** `Exec` reads "starts other
  programs, which are not limited by this list". `FFI` and the maximal set
  read "calls native code, which can do anything". Floor effects (`IO`,
  `Mem.Alloc`, `Exec.Args`) are not listed, because they carry no power the
  shell has not already given (D-AUTH-AMBIENT3's rationale, kept).
- **R5. `jet build` prints the same effect list it prints today**, from the
  R1 row, so a built binary's reach is visible at build time.

### Re-review of the redesign

- R1 can over-report: one open-world call makes the notice say "can do
  anything". That matches capslock's `UNANALYZED` and Deno's own statement,
  and the precise reference edge removes the common case (a table of named
  functions). It is accepted.
- R2 turns existing C-calling programs that carry deny rows or tight bounds
  into check errors. That is the intended effect. A binding that declares
  its row (`-[Time]>` on the import) keeps a precise row, and the census for
  the slice lists every case it moves.
- R3 makes scripts print one line on first run and stay silent after. The
  first-run cost is one stderr line, not a question.

## Gaps carried forward

| Gap | Kind | Where it goes |
|---|---|---|
| G1 | defect: written `#Import c` functions record no effect and bypass deny and bounds (P8b, P8c) | #6614 c11: `FFI.C` recorded and treated as maximal; deny and ceilings apply |
| G2 | defect: the run's effect list drops open-world rows, and function-value rows let a file read pass the old gate (P6c) | #6614 c12, the first #6614 build slice with R1 |
| G3 | ballot: #6614 c8 has no subject, because no command runs a URL or registry reference | owner question through Main: reword c8 as a rule any future remote-run command must meet, or ballot that command |
| G4 | defect: SIGSEGV in `fn_widen` returning a function value from a list (P6b, source below) | #6281 c123 |
| G5 | wording: the ballot's "most users pass `-A`" is unmeasured (C7) | Main narrows the ballot record to the sourced claims |
| G6 | migration: `jet new` scaffolds an `authority: { allow: [...] }` row, which becomes a ceiling under the ruling | #6614 c5 slice: scaffold writes no row |
| G7 | migration: a written row stops adding to the floor and becomes a ceiling | #6614 c2 and c5; whether the floor stays inside every ceiling is an owner question through Main |

P6b, which crashes with SIGSEGV in `pick.fn_widen@308 at hidden.jet:10`,
exit 139:

```jet
use files: core.files

peek: fn(x: Int) -> Int {
    text: files.read("hidden.jet") ?? ""
    text.len() + x
}

pick: fn(table: [fn(Int) -> Int]) -> fn(Int) -> Int { table[0] }

run: fn() {
    table: [fn(Int) -> Int]{peek}
    f: pick(table)
    print("v={f(0)}")
}
```

## Verdict

**D-AUTH-DEFAULT1 holds within stated limits after redesign R1–R5.** C1, C4,
C6 and C8 hold within their limits now. C2 and C3 hold once G1 and G2 are
fixed, and those fixes come first in the build order. C7 holds once narrowed
(G5). The limits are named: `Exec` and `FFI` are escape hatches, the
granularity is the effect leaf rather than the resource, and local code runs
with full authority by design. No claim fails outright after the redesign.

## Finding dispositions

<!-- audit-dispositions:v1 -->
| finding | disposition | target or reason |
| --- | --- | --- |
| G1 | card | #6614 c11 |
| G2 | card | #6614 c12 |
| G3 | card | #6614 |
| G4 | card | #6281 c123 |
| G5 | card | #6614 |
| G6 | card | #6614 |
| G7 | card | #6614 |
<!-- /audit-dispositions -->
