# CLI arg passthrough — `jet run` vs the program

**Status:** Draft plan — needs owner review (2026-06-19)
**Card:** c11

## Problem & why it matters

`jet run app.jet --port 8080` is ambiguous: is `--port` a flag for `jet`, or
for the program being run? Today Jet resolves it **silently and wrong**.

`Source/main.rs:278` builds the positional list by dropping *every* `--`-prefixed
token:

```rust
let args: Vec<&String> = raw.iter().filter(|a| !a.starts_with("--")).collect();
```

`program_args` is then `args.iter().skip(2)` (main.rs:594) — i.e. the *filtered*
list. So any flag a user means for their program is removed before it ever
reaches `program_args`, and the program sees nothing. `jet run app.jet --port
8080` runs the program with **zero** arguments; `8080` (no `--`) survives but
`--port` is gone. No error, no warning — the flag just vanishes. That is the
worst failure mode: silent.

The program reads its args through `io.args` → `jet_std_io_args()` →
`std::env::args().collect()` (`Source/Prelude/Std.rs:458`), and the built binary
is invoked with `program_args` appended (`Source/CmdCompile.rs:131`). The plumbing
to the program works; the *selection* of which argv to forward is broken.

There is already an in-repo precedent for the fix: `jet env -- cmd` forwards
everything after `--` verbatim to a subprocess (main.rs:325, the `fwd` list).

## Prior art (terse)

- **cargo** — `cargo run -- --port 8080`. Everything before `--` is cargo's;
  everything after is the program's, forwarded verbatim (including tokens that
  look like cargo flags). The dominant convention; users already expect it.
- **npm** — `npm run x -- --flag` (same rule, one extra `--` for the script).
- **go** — `go run main.go --port 8080` has *no* separator; go stops parsing its
  own flags at the first non-flag arg, which is why `go run` flags must precede
  the file. Workable but order-sensitive and surprising.
- **deno / python -m** — POSIX `--` end-of-options separator throughout.
- **Jet's own `jet env -- cmd`** — already uses `--` as the verbatim boundary.

`--` is the POSIX end-of-options marker, the cargo convention, and already live
in this codebase. No reason to invent anything.

## Proposed design (worked example)

`--` ends jet's own option parsing. Everything after it is forwarded to the
program verbatim — untouched, in order, *including tokens that look like jet
flags*.

```shell
$ jet run app.jet --small -- --port 8080 --small
#              \_______/    \____________________/
#          jet's flags        program's args (verbatim)
```

Three hops, end to end:

1. **jet keeps** `--small` (the build profile, before `--`).
2. **forwarded verbatim** past `--`: `--port`, `8080`, `--small`. The second
   `--small` is the *program's* — jet does not re-interpret it as a build flag.
3. **the program reads** them: `io.args()` returns
   `["app", "--port", "8080", "--small"]` (argv[0] = the binary name, then the
   forwarded tokens in order).

```jet
use core.io as io;

fn main() {
    let args = io.args();        // ["app", "--port", "8080", "--small"]
    print(args.len());           // 4
    print(args.at(1));           // --port
}
```

A bare `--` with nothing after it forwards an empty arg list (program runs with
just argv[0]). A bare `--small` *before* any `--` is still jet's.

### Without `--` (the common, unambiguous case)

Plain positional words after the file still forward, exactly as today, so the
simple case stays simple:

```shell
$ jet run greet.jet Ada Lovelace      # io.args() == ["greet", "Ada", "Lovelace"]
```

The `--` separator is only *needed* when the program wants tokens that start with
`--`. Beginners who never pass `--`-flags never see it.

### Project mode

`jet run` inside a project (no file given, main.rs:524) gets the same rule: the
first `--` ends jet's options, the rest forwards to the project entry binary.

```shell
$ jet run -- --port 8080      # in a project dir; forwards --port 8080 to the entry
```

### What about a `--`-flag *before* `--`?

`jet run app.jet --port 8080` (no separator) is the genuinely ambiguous case.
Two honest choices: (a) treat unknown `--`-flags before `--` as an error that
teaches the `--` form, or (b) silently forward unknown flags. The plan
recommends **(a) error + teach** — Jet's whole posture is "reject with a great
message over guess." See D-CLI1.

## Implementation sketch — file-level touchpoints

- **`Source/main.rs:246`** (`raw`): split `raw` at the first standalone `"--"`
  into `jet_argv` (before) and `passthrough` (after, verbatim). All existing
  flag scans (`--small`, `--json`, …, lines 260–273) run over `jet_argv` only.
  The `args` positional filter (line 278) also runs over `jet_argv` only.
- **`program_args`** (main.rs:594, 525): becomes the `passthrough` slice when a
  `--` was present; otherwise the current behaviour (positional words after the
  file) is preserved for the no-separator case. One code path, computed once.
- **`Source/CmdCompile.rs:131`** — unchanged; it already appends `program_args`
  verbatim to the spawned binary.
- **`Source/Prelude/Std.rs:458`** (`jet_std_io_args`) — unchanged; reads
  `std::env::args()` of the spawned binary.
- **`Source/CLISpec.rs` / `Source/CLI.rs`** — add `--` to the usage text and the
  completion script (`CLISpec.rs:196` already special-cases `--*`); document the
  separator in the `run` help line (main.rs:96–97).
- **`docs/spec/spec.md`** — one line under the CLI section documenting `--`.

No sema/codegen change. No new keyword/sigil (I7 untouched — `--` is a CLI token,
not Jet syntax).

## Test plan

- **Integration (`tests/` driver):** `jet run fixture.jet -- --port 8080 x`
  where `fixture.jet` prints `io.args()`; assert output is
  `["fixture", "--port", "8080", "x"]`.
- **Verbatim flag-lookalike:** `jet run fixture.jet --small -- --small`; assert
  the program receives `--small` *and* the binary was built with the small
  profile (jet kept the first, forwarded the second).
- **Bare `--`:** `jet run fixture.jet --` → `io.args() == ["fixture"]`.
- **No separator, positional:** `jet run greet.jet Ada` still forwards `Ada`
  (regression guard for the simple path).
- **Project mode:** `jet run -- --port 8080` in a project fixture forwards to the
  entry.
- **If D-CLI1 picks error-on-ambiguous:** re-bless the E2102 ui snapshot for
  `jet run app.jet --port` (no `--`) with the extended Fix line, what/why/fix per
  diagnostics.md.
- **Example (I5):** `examples/features/NN_cli_args.jet` + expected output, golden.

## Risks & invariant check

- **I1/I2/I3** — untouched; CLI plumbing only, no codegen, rustc never involved.
- **I4** — if D-CLI1 chooses the error-on-ambiguous arm (A), it reuses the
  existing E2102 ("isn't a flag jet understands", main.rs:210) — no *new*
  diagnostic code, just an extended Fix line teaching `--`; re-bless its ui
  snapshot. No new code is added on any arm.
- **I7** — `--` is a CLI argument, not a Jet token; `Source/Syntax.rs` unchanged.
- **I8** — no language feature added; this removes a silent-data-loss bug. Strong
  ratchet fit.
- **Compat risk:** today `jet run app.jet --foo` silently drops `--foo`. After
  this, that same command either errors (D-CLI1 rec) or still drops it
  (alternative). Either is strictly better than silent loss, but it *is* a
  behaviour change for anyone who (unknowingly) relied on the drop. Low risk —
  the current behaviour is a bug.

## Open decisions

1. **D-CLI1** — what happens to a `--`-flag *before* the separator that jet
   doesn't recognise (`jet run app.jet --port`)? Error-and-teach vs
   silent-forward vs forward-with-warning. (Card below.)
2. Should `jet test` / `jet build` accept `--` too, or only `jet run`? (Lean:
   `run` and `test` yes — `test` programs take filters; `build` produces no
   running process, so no.) Not a syntax decision; note for the implementer.

## Proposed decision card(s)

### D-CLI1 — Unknown `--`-flag before the `--` separator (rec A)

`--` cleanly forwards everything after it. The remaining question is the
*ambiguous* case: a `--`-flag that appears **before** any `--`, which jet doesn't
recognise as one of its own. What should `jet run app.jet --port 8080` do?

- **Option A — Error and teach (recommended).** Reject the unknown flag with a
  diagnostic that names the `--` form. Honest, no silent loss, teaches the
  convention once. The machinery already exists: `check_flags` (main.rs:200)
  already errors on unknown `--`-flags via E2102 — this just extends its Fix line
  to point at `--` for `jet run`.

    ```shell
    $ jet run app.jet --port 8080
    Error [E2102]: `--port` isn't a flag jet understands
     Why: flags before `--` belong to jet; everything after `--` is forwarded to your program
     Fix: jet run app.jet -- --port 8080
    ```

- **Option B — Silently forward unknown flags.** Any `--`-flag jet doesn't know
  gets forwarded to the program. Convenient, no `--` needed — but a *typo'd* jet
  flag (`--smal`) is then silently handed to the program instead of being caught.

    ```shell
    $ jet run app.jet --port 8080      # program gets --port 8080; no error
    $ jet run app.jet --smal           # typo silently forwarded, not caught
    ```

- **Option C — Forward with a one-time warning.** Forward the unknown flag but
  print a lint-style note suggesting `--`. Middle ground; adds noise to a common
  path and still forwards typos.

    ```shell
    $ jet run app.jet --port 8080
    Warning: forwarding `--port` to your program; use `jet run app.jet -- --port 8080` to be explicit
    ```

**Recommendation: A.** Jet rejects-with-a-great-message over guessing (philosophy
+ I8), and the E2102 path that does it already exists. `--` is one keystroke,
already live in `jet env`; teaching it once at the first ambiguous flag is cheaper
than the class of silent typo-forwarding bugs B admits (a typo'd `--smal` would be
handed to the program instead of caught). C is the fallback if the owner wants
zero friction on the common path, but A is the Jet-consistent call.
