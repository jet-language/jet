# Command-line interface

Jet's CLI is a command dispatcher, not a second language surface. This page is
for users choosing a command and for scripts that need the stable spelling of a
flag. The executable truth is the generated output of

```sh
target/debug/jet help
```

and the command table in
[`Compiler/JetFoundation/Source/Registry/CLICommands.jet`](../../../Compiler/JetFoundation/Source/Registry/CLICommands.jet).
The Rust host keeps commands in
[`crates/jet-foundation/src/CLICommands.rs`](../../../crates/jet-foundation/src/CLICommands.rs)
and flags in `crates/jet-cli/src/CLI.rs`; `node Compiler/Bootstrap/check-command-table.mjs` compares the assembled tables.
Run `jet help <command>` for the complete options for one command; do not infer an
option from an older guide.

Help and version are human renderables, not machine records: `jet help --json`
and `jet version --json` succeed with no output. Diagnostic essays instead
use the `jet.status/v1` `explain` action under `--json`. `jet emit --rust --json`
still writes the generated Rust verbatim; its machine mode affects diagnostic
failures, not the emitted program. C and C++ driver aliases own their raw
arguments (including `--help` and `--version`) before Jet parses global flags.

## Project jobs

Write a named chore as `#Job fn deploy(region: String) { … }`, then run
`jet deploy --region eu`. Jet flags come before the job name:
`jet --interpret deploy --region eu`. Every word after the name belongs to
the job; a separator immediately after the name is optional.

Lookup is commands, visible project jobs, retired/moved spelling diagnostics,
source paths, then `jet-X` on PATH. Jobs cannot reuse any command in the
shared command table (E0928); retired words are not reserved. A job wins over
a same-named folder, matching `.jet` file, or PATH program. A word ending in
`.jet` always names a file, and `jet run deploy` explicitly runs a path.
`.Internal` jobs cannot be selected from argv. Outside a project the job
lookup is skipped. Unknown words report E2101 and suggest commands and jobs.

`jet jobs` lists jobs; its `--graph`, `--status`, `--explain`, and `--watch`
views remain available. `jet jobs deploy` is an error teaching `jet deploy`,
not an alias. `jet run <entry> -- deploy` and `<built-program> deploy` use
the same checked job dispatch.

## Root command index

The root index below mirrors `target/debug/jet help` in this checkout. It lists
the most used commands first, in the order `node Tools/cli-census/census.mjs`
measures from the guides, examples, and tests; `jet help --sort az` lists the
same commands A to Z and `jet help --sort za` Z to A:

```text
jet run [<file.jet|dir>] [--no-prepare] [-- <args>] Run a program or project
jet build [<file.jet|dir>] | build --verify <receipt-id> Create a native executable
jet check [<file.jet|dir>]           check code without creating a binary
jet inspect <command>                Explore code, builds, packages, and bindings
jet explain <CODE|FACT> [file] | explain --cost <file.jet> | explain --reload <file.jet|dir> Explain a diagnostic code, build fact, generic-module value, or typed cost
jet dev [<file.jet|dir>] [--canvas|--app <function>] [--share <loopback|lan>] [--token <token>] [-- <args>] Watch and run a program; optionally open Canvas or a local app
jet test [<file.jet|dir>] [<filter>] [--watch] [--fresh] [--docs] [--where=<expr>] [--capture=<failed|all|none>] [--browser=<chromium,firefox,webkit>] [--browser-retries=<n>] [--browser-reporter=<text|json|html>] [--browser-ui] [--browser-visual] [--browser-trace] [--browser-scaffold=<name>] [--grade=generated] [--iterations=<n>] [--time=<s>] [--seed=<n>] [--corpus=<dir>] Run tests
jet fmt [args]                       Format Jet and configured project files
jet fix <file.jet|dir>               Apply safe automatic fixes, including `fix memory`
jet debug [<file.jet>] [--record=NAME|--replay=NAME] [--dap] [--raw-frames] Debug a program from Jet source
jet self <command>                   Manage the Jet installation and editor tools
jet os <command>                     Manage Jetos machines and images
jet prove [args]                     Create a proof report for code and tests
jet fetch [args]                     Download locked dependencies
jet new <name> [--template cli|ui|web|overrides] | new service|route|job|migration <name> [--path <path>] [--route <path>] [--model <name>] [--up|--sql <SQL>] [--down <SQL>] [--risk <note>] [--lock <shared|exclusive>] [--version <n>] [--preview|--apply|--remove] Create a Jet project or backend source scaffold
jet registry <command>               Publish and manage packages
jet update [args]                    Update dependency or toolchain pins
jet env <command>                    Open the project development shell
jet help [<command>] [--sort frequency|az|za] Show command help
jet perf <command>                   Collect and inspect performance traces
jet image [args]                     Build a declared container image
jet eval <file.jet|expression>       Evaluate pure Jet and print the value (`--json` for JSON)
jet init [args]                      Create package settings in this directory
jet repl [<file.jet>] [--project <dir>] [--console] [--sandbox data] [--console-ttl <milliseconds>] [--allow=<RIGHTS>] [--deny=<RIGHTS>] Try Jet code interactively
jet jobs [--graph|--status|--explain|--watch[=<on|off>]] [<name>] List, inspect, or watch named project jobs
jet budget [args]                    check performance limits or update baselines
jet audit [args]                     Inspect implicit copies, exercised memory witnesses, or dependencies
jet package --kind <desktop|game> --target <linux-appimage|macos-app|windows-msix> [--executable <path>|<source.jet>] [--output <path>] [--profile <dev|release|name>] [--phase <build,cook,stage,package,export,deploy,run>] [--backend <aot>] [--renderer <headless|raylib>] [--cook-mode <fast|reproducible|scripts-only>] [--export-preset <default|store|headless>] [--deploy-to <path>] [--crash-reporter <off|on|opt-in>] [--crash-consent <not-requested|granted|denied>] [--dry-run] [--explain] [--resume|--cancel] [--clean|--scripts-only] [--run-now] [--package <id>] [--name <name>] [--version <version>] [--icon <path>] [--icon-format <png|icns|ico|svg>] [--icon-size <pixels>] [--publisher <name>] [--description <text>] [--update-channel <channel>] [--update-url <url>] Create a desktop or game application bundle
jet flash --target <board.name> [--image <firmware.elf>] [--audit <target.json>] [--adapter <probe-rs|openocd|emulator>] Flash firmware to a target board
jet fuzz <file.jet> [<test>]         Find failing inputs for property tests
jet import <language> <dir> [--dry-run|--update] Convert supported source code into editable Jet
jet status [<file.jet|dir>]          Show what the project has proved
jet version [args]                   Show the Jet version
jet gc <command>                     Show values moved into automatic memory management
jet learn [--check] [--watch|--watch=off] [--json] [--quiet] [--color[=<mode>]] Practice Jet with offline code exercises
jet notebook [args]                  Open a Jet notebook (.jetnb) or Jupyter adapter
jet bind <name> [--shape automatic|native] [--freeze]
bind --policy automatic|frozen
bind <name> --update --preview
bind <name> --update --accept <candidate-digest> Resolve and record a checked foreign binding plan
jet cc [options] <sources>           Compile and link C with the pinned Jetpack toolchain
jet add [args]                       add and download a dependency
jet c++ [options] <sources>          Compile and link C++ with the pinned Jetpack toolchain
jet clean [args]                     remove unused package-store data
jet emit [args]                      Print generated build output
jet lint --a11y|--complexity|--cost <file.jet> Run optional code-quality checks
jet trust [args]                     Review or change trusted authority
jet review <base.jet> <head.jet>     Review meaning, authority, and proof changes
jet search <query>                   Search the local package catalog
jet cache <command>                  Manage the machine-wide artifact store
jet doc [--json|--check] [<file.jet|dir>] Generate reference documentation
jet split [args]                     Extract closed Package facts into Configs or members
jet diff [args]                      Compare two Jet programs by meaning
jet remote [args]                    Manage host-owned remote builders
jet merge [args]                     Merge Jet programs without losing code structure
jet project <command>                Inspect project files and modules
jet test-compare <corpus.json> [--relation=<name>] [--json] Compare one recorded observation corpus against its relation
jet find [--effect <effect>] [--example <input -> output>] [<query>] [<file.jet|dir>] Find code by type, effect, or example
jet remove [args]                    remove a dependency
jet db [PATH] [--query SQL | --script PATH] Run a bounded SQL console and manage database migrations
jet fill <file.jet[:line]>           Propose checked code for typed goals
jet generate <GeneratorJob> [--apply|--dry-run] [--entry <file.jet>] [--json] Run an explicit source generator with authority and a receipt
jet shared-store <command>           Manage the optional shared package broker
jet try <plan.json>                  Speculatively apply a plan and re-check its claims
jet Fold [args]                      Reverse a recorded Package source transition
```

The `bind` shorthand accepts the explicit binding-plan form shown by help:
`bind <name> --update --preview` or
`bind <name> --update --accept <candidate-digest>`. Foreign-header bind-in
commands remain under `jet inspect bind`.

`fix memory` uses exercised runtime witnesses, not whole-program memory proof.
It applies only a single repair whose original byte span still matches the
source (or recognizes that the replacement is already present). Overlapping,
stale, out-of-root, and ambiguous suggestions remain options. `--dry-run`
reports prospective repairs without writing files; its JSON `changed` fields
remain false.

`fill` reads typed goal facts from the checked program. A goal with no known
expected type reports `(unknown)`; the checker's continuation type does not
turn that into an inferred contract. Proposals include matching parameters,
zero-argument functions and type-appropriate literals, but a proposal is
listed only after replacing that goal and checking the whole source again.
The optional `:line` suffix filters the report, not the checked program.

Structural `diff` and `merge` align checked definitions rather than source
lines. A rename is recorded intent only when a semantic-operation receipt
names the compared file and its before/after byte checkpoints. Hand edits
do not inherit stale rename intent. A merge conflict leaves the output
unwritten; a clean candidate must parse and pass semantic checking before
publication.
## Command groups

The generated help groups subcommands as follows:

- **Registry:** `publish`, `yank`, `keygen`, `key backup`, and `vendor`.
- **Db:** bounded SQL plus `migrate new`, `preview`, `status`, `target`,
  `apply`, `step`, `rollback`, and `resume`.
- **Inspect:** `types`, `rights`, `claims`, `shapes`, `accel`, `decisions`,
  `structure`, `build`, `gates`, `graph`, `query build`, `explain-build`,
  `compiler`, `impact`, `provenance`, `digest`, `env`, `semindex`, `output`,
  `expand`, `schema`, `codemod`, `audit`, `sbom`, `bind`, `logs`, `info`,
  `outdated`, and `reserved`.
- **Project:** `parts`.
- **Self:** `toolchain`, `update`, `doctor`, `completions`, `man`, `devtools`,
  `lsp`, and authority-bound `exec`.
- **Environment and stores:** `env test|hook|sync|info`, `shared-store
  install|enroll|status|broker`, and `cache status|prune|limit`.
- **Jetos and performance:** `os push|bridge|services|config`, `gc report`,
  and `perf run|test|attach|view|compare|export`.

Use `jet help registry`, `jet help inspect`, or `jet help <subcommand>` when a
script needs the complete generated synopsis rather than this index.

`find` and symbol help read source-facing names from Sema's checked name
ledger. Unambiguous visible names stay short; ambiguous names retain their
canonical path. Generated inline and generic-module names project to their
authored member path without changing package-qualified semantic identity.

Doc-mode compiler results retain the selected target triple and layout
capabilities, typed layout reflection values, and transitive memory-denial
projections from the checker that diagnoses E0921. Prepared `@origin` reads
retain their checked optional `OriginInfo` values on the owning function;
untracked or moved locals have no readable origin.

## Shared option spellings

The options below are intentionally named exactly as in `target/debug/jet help`:

- `--target` selects a triple, `board.<name>`, or named database target;
  `--profile` selects `release`, `debug`, `ci`, `hardened`, or a named bundle.
- `--allow=RIGHTS` and `--deny=RIGHTS` set effect roots or leaves for
  `run`, `build`, `jobs`, `repl`, and `db`; `--offline` reuses only cached
  signed toolchain and package data.
- `--locked` requires locked dependency and provenance facts; `--fixtures`
  selects an explicit fixture bundle.
- `--lib` makes `build` emit the native Library and C header; `--output` selects
  a named output; `--small` favors a smaller binary.
- `--json`, `--quiet`, `--color`, and `--verbose` select output detail.
- `--watch`, `--fresh`, `--docs`, `--coverage`, `--where`, and `--capture`
  control test or development execution; `--browser` selects browser engines.
- `--explain-partition` is the web-target build option that shows JavaScript or
  WebAssembly assignment.
- `--threads N` caps the compiler at N threads on `build`, `run`, `check`,
  `test`, `dev`, and `inspect` (D-JOBS1=A); without it the compiler admits
  threads automatically up to the reported cores. It never limits the threads
  of the program jet builds or runs, every value gives the same diagnostics
  and output bytes, and `build --verbose` reports the cap it used. No
  environment variable sets it. `--jobs` and `-j` are usage errors (E2102)
  that point to `--threads`, because `jobs` names project jobs.

For C and C++, the driver-specific options are `--project-root`, `--build-root`,
`-c`, `-o`, `-MMD`, `-MD`, `-MF`, `-MT`, reserved `--sysroot`, `-I`, `-D`, `-L`,
`-l`, `-std`, `-dumpmachine`, `-print-sysroot`, `-dumpversion`, `-v`, and
`--target`. The exact acceptance and forwarding behavior is documented in
[CC driver hosts](cc-driver.md).

## Jet-hosted `check`, `build`, `run`, `test`, `new`, and `fmt`

The Jet-hosted CLI ([`Compiler/JetCli`](../../../Compiler/JetCli)) runs
`jet check <file>`, `jet build <file>`, `jet run <file>`, and
`jet test <file>` through the Jet
compiler (`jet_driver_compile`). It gives the compiler one authorized source
root: the nearest directory at or above the file that holds `package.jet`, or
the file's own directory for a loose file, with every `.jet` file under it (dot names,
`target`, `build`, `node_modules`, `bin`, and nested projects stay out) and the
generated C/C++ binding caches. The Core library bodies are read from the Jet
toolchain source tree named by `JET_TOOLCHAIN_ROOT` (the directory that holds
`Core/`); the Core row table and the canonical effect source are generated
into [`CoreSources.jet`](../../../Compiler/JetCli/Source/Cli/CoreSources.jet)
by `node Compiler/Bootstrap/generate-core-sources.mjs`, which
`node Tools/agent/check-core-surface-ledger.mjs --write` runs and `--check`
holds current.

`jet build` writes `.jet/build/<stem>` in that source root. The default and
`--profile debug` builds use the Jet-native backend (`lir_lower_program` and a
static x86-64 executable); a program that backend cannot lower yet is refused
with E2104. `--release` (or `--profile release`) compiles the emitted Rust with
rustc and the release profile flags. The Rust file starts with the same
runtime/Core prefix the Rust compiler emits (MIRRust `emit_mir_runtime_text`):
[`Emit/RuntimePrefix.jet`](../../../Compiler/JetCodegen/Source/Emit/RuntimePrefix.jet)
assembles it from the Prelude and Foundation `.rs` sources under
`JET_TOOLCHAIN_ROOT` (which must therefore also hold `crates/`), with the
artifact's runtime parts, the test-harness gate, and the release devtools
policy (`local` inspection for `--release`, development for a test harness).
Other build flags and profiles are refused with E2104 until they are ported.

`jet run <file>` (and the `jet <file>` shorthand) builds the same executable
with the same profile rules, then runs it on the caller's stdin, stdout, and
stderr and exits with its status (1 when a signal ended it). The program's
arguments are the words after the first standalone `--`, or else the
positional words after the file. Advisory lints stay hidden unless `--verbose`
or `-v` comes before `--`; a verbose run reports them before the program
starts. Run flags other than `--release`, `--profile`, `--verbose`, `--quiet`,
`--color`, `--threads`, `--allow`, and `--deny` are refused with E2104 until they are ported.

Before `jet build` or `jet run` writes anything, the Jet host checks the
application authority ([`Authority.jet`](../../../Compiler/JetCli/Source/Cli/Authority.jet)):
the checked entry's solved effect row against the beginner floor (`IO`,
`Mem.Alloc`, `Exec.Args`), the entry package's `authority.holds` (from
`package.jet` or the entry's leading inline `package { … }` block, which the
Jet loader reads with the same field parser and masks before parsing), and the
`--allow=`/`--deny=` rows of the invocation. A denied or undecided right is
E1803, exit 1; a file with no package manifest leads the Fix with the exact
`--allow` command. The interactive confirmation (D-SCRIPT-CONFIRM1), authority
receipts, and lint-policy enforcement are not ported yet: the Jet host always
answers as a non-interactive run.

`jet test <file> [<filter>]` compiles the file in test mode: each named
`#Test` body is checked and lowered as a synthetic function (a property
test's ungeneratable parameter types are E0613, a file with no named test is
E0601), and the artifact is a TestExecutable whose harness owns `main`. The
harness is built through rustc as `.jet/build/.test_<stem>.<pid>` (the
Jet-native backend has no harness entry yet) and run with `JET_TEST_CAPTURE`,
`JET_TEST_FILTER`, `JET_TEST_SHUFFLE_SEED`, `JET_TEST_SERIAL`,
`JET_TEST_TRACE_TIERS`, `JET_UPDATE_SNAPSHOTS`, and `JET_PROJECT_ROOT`. Its
captured output is shown under `--capture=all`, or for a failed run under the
default `--capture=failed`; the exit is 0 when the harness passed, else 1. It
honors `--capture`, `--filter`, `--shuffle=<seed>`, `--serial`,
`--trace-tiers`, `-u`/`--update-snapshots`, `--fresh`, `--show-default`,
`--release`, `--profile`, `--verbose`, `--quiet`, `--color`, and `--threads`.
Directory and package targets, doctests, a `fn test` override, a bare
`--shuffle`, and the other test flags are refused with E2104 until they are
ported. The test-result cache, the evidence report, and the package claims
floor are not ported yet: every run compiles fresh, and the outcome is the
harness exit (the Rust host's outcome for a package without a claims floor).

`jet new <name> [--template cli|ui|web|overrides] [--annotated]` writes the
same project the Rust host writes (CmdCompile.rs `run_new`): `package.jet`
with the template's authority holds, `.jet/lock` with the running toolchain
pin and the build stamp (the git revision when the folder is inside a
repository), the template's `run.jet`, the four comment-only `@*.jet` command
homes for `overrides`, and `.gitignore`, then the status lines on stderr
(muted by `--quiet`). A bare `jet new` prints the command help with exit 2.
The backend scaffolds (`jet new service|route|job|migration`) are refused with
E2104 until they are ported.

`jet fmt [paths…] [-]` formats `.jet` sources with the Jet port of the
formatter ([`Compiler/JetParser/Source/Formatter`](../../../Compiler/JetParser/Source/Formatter),
one module per Rust `Formatter` module) the way CmdCompile.rs `run_fmt` does:
no paths walk the project root (the nearest `package.jet` directory, else the
working directory), a directory is walked sorted with the ignored directories
and snapshot trees skipped, every file is formatted before any is written, and
one read or parse failure reports every failure with exit 2 (exit 1 under
`--check`). `--check` lists the files that would change (root-relative,
sorted) and exits 1; `--diff` adds unified diffs; `--dry-run` is both; `-`
formats stdin to stdout under the `--stdin-path=` label; `--simplify` enables
the ratified rewrites. The retired-spelling status lines go to stderr unless
`--quiet`. `package.jet` and `Config` record sources go through the typed
Package formatter (the Jetpack Package model, referenced into the Compiler
unit from `Jetpack/PackageModel`): an unreadable manifest stops the run with
its E1206-family diagnostic, and a layout the model declines (authored
comments) is left as written. `--changed` formats the `.jet` files git reports
changed against `HEAD` or staged (exit 2 outside a git worktree).
`--explicit-copies` checks each file, then writes `~` at every implicit copy
site: the Safe `~` edits sema gives the same source under
`#Policy(copies: .Explicit)`; marks that would not parse report E0122.
With `--json`, a clean `--check` emits the `fmt` status envelope; dirty
checks emit `status: "dirty"` with paths, or path/diff objects with `--diff`.
Read and parse failures stay on stderr in machine form, and no file is
written when any preflight fails. Ordinary write mode emits no success
envelope; stdin mode still emits the formatted source, not a JSON wrapper.
`--lang <language>` evaluates the `env.jet` above the working directory with
the Jetpack environment model (`--preset`/`--env` select as in `jet env`),
takes the selected environment's `formatter:` package, stages the matching
`.<language>` files (the environment root, or the named paths) in a
temporary directory, and runs `jetpack env … -- <formatter> <staged…>`;
`--check`/`--diff`/`--dry-run` report the files whose bytes changed (exit 1),
otherwise those files are written back. No formatter declared is E1340.
