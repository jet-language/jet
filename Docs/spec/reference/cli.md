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

`jet self devtools new-ui <name>` writes `tests/ui/<name>.jet` and its
diagnostic snapshot using the same compiler and renderer as the UI harness.
Only the source-file collision is refused; an existing `.stderr` is replaced.
`jet self devtools check-fixture-paths` checks literal fixture paths in
`tests/**/*.rs`, excluding diagnostic display labels and dynamic templates.
Both commands retain their direct confirmation output under `--json` and
`--quiet`; these flags do not hide generated artifacts.
Missing-path diagnostics use deterministic directory order; Rust's OS directory
order is unspecified. Path occurrences, including duplicates, remain counted.

`jet inspect schema status` reads published snapshots and runs the canonical
load-only E0910 advisory diff; an unavailable source bundle does not prevent
listing its snapshots. `schema squash --before <version>` snapshots the current
published declarations and records the cutoff without editing source. Only
successful snapshot writes are counted. These commands keep their direct
human report under `--json` and `--quiet`; neither mode creates a schema-status
JSON protocol.

`jet self completions <shell> --for <program>` reads embedded command metadata
without executing the program. Shell registration uses its basename, with
shell-specific quoting; only schema-known inputs and commands are candidates.
The reader opens once without blocking, checks that opened file is regular,
and bounds the read to 512 MiB before verifying the record format and digest.
Offset overflow follows the CLI's compile-target pointer layout, not the
inspected executable's word size. Metadata failures always use direct E2103
stderr; completion arguments retain their exact command-specific grammar.

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

The compiler request selects a job or generator by its declared callable in
the authorized entry module. It does not rename that function or manufacture
`fn run`. Evaluation uses the same selection and checks transitive purity
through sema's checked call graph before execution.

## Published schemas

`#PublishedSchema` records have a lockfile-style snapshot in
`.jet/cache/schema/<Type>.snapshot`. The canonical sema codec records the
format version, type, published version, optional `squashed_before` cutoff,
and declaration-order field names and types. Field reordering is not breaking.
Removing a field, changing its type, or adding a required field needs declared
migration intent; invalid migration operations also report E0910 before
codegen. A type change requires an inline converter or an `impl Old -> New`.

`inspect schema status` uses the same sema diff over authorized load-only
declarations, intentionally without a full build's populated trait registry.
The full-build check remains authoritative. Snapshot reads honor
`JET_SCHEMA_CACHE_DIR`; publication and squash writes always use the project's
canonical cache directory. Source-only queries retain the loader's original
program, physical path, source bytes, and optional selected entry Package
version; they do not substitute a semantic check or reparse a manifest in the
CLI. Squash uses that declared version (falling back to `0.0.0` only when it
is absent).

## Executable command metadata

Application completion reads the canonical v8 `JetCommandSchema` carrier,
including input defaults, subcommands, and all job execution metadata. The
Foundation byte reader recognizes ELF (both classes and byte orders), PE,
Mach-O (thin and universal), and Wasm custom sections. It verifies the record
length, digest, version, and field bounds before publishing a schema; missing,
duplicate, unsupported-format, and malformed records remain distinct errors.
PE permits zero alignment padding but not another record or nonzero trailing
bytes. Every universal Mach-O slice must contain the same single record.

File-size arithmetic is bounded by `JET_COMPILER_TARGET_USIZE_MAX`, a
compiler-private constant folded from the selected build target's pointer
layout. It is not inferred from the inspected executable's ISA and requires
no runtime pointer-width leaf. The explicit bound in the byte-reader API
also permits portable boundary fixtures. Completion candidates retain written
order, positional ordering, and duplicate visible job names; internal jobs do
not become argv-reachable.
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
Source repairs preserve permissions and publish by a staged, flushed rename
relative to the held source directory. Default data-binding outputs are
published under the selected workspace/repository/package root's
`.jet/bindings`, with collision checks performed under that directory's lock.
Root discovery uses checked workspace declarations, not filenames alone,
then the enclosing repository or package, and finally the source's canonical
directory. Missing workspace declarations are absence; malformed, ambiguous,
linked, or unreadable authority metadata remains a diagnostic.

Receipt objects and their append-only section records are immutable,
content-authenticated files under `.jet/receipts`. The record-index JSONL
snapshot under `.jet/records` is reloaded while holding its directory lock
before each mutation and replaced atomically with mode `0600`. FIFO eviction
removes only unreferenced disposable groups; removed artifacts are cleaned up
after the new index is published, without following links.

`jet test-compare <corpus.json>` compares recorded reference and candidate
observations using the shared finite-sample verdict. It records the canonical
comparison payload as an immutable receipt section, consumes its protected
evidence report, and publishes comparison/receipt index links together.
Evidence publication selects the project from the working directory; receipt
publication selects it from the corpus target. Generated inputs participate
in receipt invalidation but not the canonical target-input identity. Optional
history failures do not change the comparison's verdict or exit status.

`jet remote bind`, `list`, and `remove` manage host-owned builder bindings
under `$XDG_CONFIG_HOME/jet/remote-bindings` (or `$HOME/.config`).
Registry records contain only a credential-provider path, never its bytes.
Binding and loading validate the provider through held, no-follow directory
reads; registry writes are staged, flushed, and atomically replaced with
mode `0600`. Listing is deterministic and JSON omits unreadable bindings.

`gc report` reads the selected project's bounded, complete GC identity trace
without running a producer. It rejects missing, stale, unsafe, incompatible,
oversized, malformed, or incomplete traces with E2110. Counters must match
unique recorded identities; the report never estimates dropped promotions.
`JET_GC_TRACE` selects an explicit trace path. `--json` uses the `gc.report`
status envelope with the same checked sites and identity evidence.

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
`expand --facts memory`, `layout`, and `origin` render those retained facts
as text or canonical JSON. A layout without a physical-byte guarantee keeps
unknown byte fields absent and reports E0959; it does not estimate them from
the host. The origin lens reads the checked prepared value, not a source-text
guess about whether a value was tracked.
Convention routing reads complete retained directory listings from the
authorized compiler request, including empty directories. It never invents
directories from source-file names or reopens ambient filesystem paths.

### Checked build inspection

`inspect build`, `inspect graph`, and `inspect query build` evaluate the
selected package's checked `fn build` in one invocation-owned context. They
finish and validate its graph without executing actions or publishing generated
files. An absent root build entry reports the default pipeline (`build: null`
in the `inspect.build` status envelope), not an invented empty BuildPlan.
Target and action explanations use the checked declaration; file explanations
use its retained ownership, consumers, targets, and generated-source digest.
Unobserved action-cache status remains `unknown`.
The default-pipeline path still checks the package semantically. Failed queries
use the standard `diagnostics` JSON status envelope or human problem-count
report, with advisory lints shown only under `--verbose`/`-v`; internal query
failures retain the compiler-error stream and exit status.

Build constructors return typed failed values on fallible calls. Their
`BuildError` variant, JSON fields, and original source span survive catching
and propagation; declaration failures do not prematurely terminate an otherwise
recoverable build. An uncaught returned build error becomes a structured
diagnostic. Authorization/`#Impure`, argument-shape, and unavailable-service
failures remain diagnostics rather than fabricated BuildError values.

Execution-node explanations require authenticated build-record evidence for
duration and rebuild reason. Static graph keys do not certify a `first-run`,
a duration, or a cache hit. Missing runtime evidence and uncertified comptime
input coverage remain explicit refusals.


## External formatting

`jet fmt --lang nix` selects the realized environment's formatter package.
Inputs are staged as byte-for-byte copies; the environment child returns to
Jet before any result is reported or published. Jet reads every staged output
before writing originals. `--check`, `--diff`, and `--dry-run` leave originals
unchanged and return a user-error status when formatting would change them.
Diffs display invalid UTF-8 with replacement characters; writeback preserves
the actual formatter bytes. `--json` uses the shared `fmt` status envelope for
clean, dirty, and diff results. A failed formatter never publishes staged
changes. A later write failure does not undo earlier successful writes.

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

The native bootstrap host also authorizes loose source roots without creating
a manifest. A missing `package.jet` preserves the compiler's source-root
namespace semantics; retired manifests, symlinks, and other authority errors
are not treated as missing. Existing manifests and source files retain their
checked handles and are revalidated before invoking the compiler.

`jet build` writes `.jet/build/<stem>` in that source root. The default and
`--profile debug` builds use the Jet-native backend: `lir_lower_entry` lowers
the functions `fn run` reaches to a relocatable object, and the in-process
linker binds it to the runtime pack shipped beside `jet` into a static x86-64
executable. A program that backend cannot lower yet is refused
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

The Jet-hosted `lint --a11y` and `lint --complexity` leaves use the checked
compiler diagnostics. Accessibility findings exit 1 and go to stderr, as
diagnostic JSON Lines under `--json`; a clean machine invocation emits no
bytes. Check errors use the `diagnostics` status envelope on stdout.
Complexity scores count parser control-flow nodes, enclosing structures,
mixed boolean sequences, and direct recursion. Reports sort by descending
score, source position, then name. `--max=<n>` accepts a non-negative 32-bit
budget; its JSON result remains an array, and an exceeded budget exits 1.
Without a budget, the `lint.complexity` machine envelope also carries checked
architecture advice and check provenance. Shared-callee and consumer-agreement
candidates retain definition/call-site spans, effects, behavioral-check
statuses, and uncertainty; they neither execute user code nor apply a refactor.

`inspect structure` presents the compiler's published name facts and the
structure slice of its gate ledger; it does not infer missing facts by scanning
source text. Frontend failures can publish a structure report before the
diagnostic report, so the machine stream can contain a successful envelope
followed by a failed one. `--core` presents the canonical bootstrap order,
root types, and module/type export declarations without compiling the entry.
Its ordinary inspect target/project resolution still applies.
The Jet semantic pass publishes unused-import, private-function, and
package-export liveness verdicts in source/span/code/name order, with the same
`unreachable` status and collector detail as the diagnostic. Resolved user and
Core lifecycle markers publish the consumer's site, package-edition status,
and replacement detail. Once checked modules exist, failed Doc compilations
retain these facts as well as the earlier resolved import edges.
The hosted Core/Structure catalog is regenerated from `Prelude/Core.jet`
and `Prelude/Facts.jet` with
`node Compiler/Bootstrap/generate-structure-registry.mjs`; `--check` detects
a stale generated projection.

Offline syntax explanations read `SyntaxDictionaryData.jet`, a Jet-owned
dictionary of keywords, markers, types, built-ins, operators, and sigils.
A token can have several position-dependent meanings, examples, consequences,
and related diagnostic codes. Lookup ignores ASCII case; `--verbose` adds the
registry name and decision only in text mode. Unknown syntax-like queries
produce E2106 with a nearest registered spelling; package references are not
classified as syntax typos. Standalone policy essays use the Foundation policy
registry after syntax lookup. Effective source-site policy and build-fact
writer-chain queries remain distinct loader-backed operations. Loaded policy
queries do not require semantic compilation of callable bodies. The compiler
prepends the selected entry package's policy declarations to each loaded user
module and resolves the shared package/module/function/block ladder before
publishing those facts. Package provenance names the physical `package.jet`
or inline Package carrier; source-authored provenance names the same relative
display path as the loaded module (an external module keeps its absolute path).

Unsafe-gate inspection reads the parser's audited regions and scoped policy
declarations. It retains each low-level operation, its typed obligations
(`valid_ptr`, `aligned`, `no_alias`), the immediately following assertion, and
the policy provenance. Nested unsafe regions own their proofs independently;
an assertion cannot discharge an operation across a region boundary.

The checked documentation seam retains a shared `GateLedger` alongside its
name and memory facts. Gate readers select these rows rather than interpreting
source strings: security gates sort before knowledge and structure gates,
duplicate facts merge all provenance, and unsafe rows retain their operation
proofs and diagnostics. Package policy, invocation choices, and external
lock/trust/forced-build writers enter through the driver's typed gate inputs.

The hosted `cache status`, `cache prune --to <size>`, and
`cache limit --host <size>` commands use Jetpack's existing machine-wide
`ArtifactStore`, including its atomic host policy, verified namespaces, access
journal, live-owner leases, and deterministic pruning order. Text and
`jet.status/v1` JSON retain the Rust command's fields and size formatting.
The CLI does not introduce another store or emulate cache mutation.

Environment inspection's `.Oci` image/package cross-check now uses Jetpack's
authority-backed `PackageFacts.load` reader, including inline package conflict
checks and composed manifest facts. A present package is no longer refused by
a compiler-only loader substitute; absent and malformed package outcomes keep
the environment model's existing E1267 behavior.

`jet self toolchain` reads the nearest checked package authority and reports
its toolchain pin, locked version, object identity, and executable realization
state through Jetpack's shared pin reader. It does not resolve or move a pin.
The report is a human renderable: `--json` suppresses successful report bytes,
while missing or malformed package authority remains an error.

Offline `jet explain` queries also project registered fact declarations and
corpus truths. Fact essays state the row's attachment target, safe direction,
written loosening gates, and read-only prover where one is declared. Corpus
truths state their published home, renderers, and single-home guard. Successful
syntax and standalone policy lookup still win; source-backed build-writer
queries remain separate and never become a generic registration essay.

Source-file policy and marker-site essays resolve the loaded declarations with
the shared scoped-policy resolver. Marker locations use UTF-8 byte offsets;
an out-of-range line selects the end of the source. Their source prerequisite
is the authorized load-only query, not a full semantic check: an undefined
function in an otherwise parseable file does not prevent a policy essay.

`jet self devtools new-example <topic>/<name>` writes the feature example and
its expected output using the golden harness's existing directory layout.
Existing example source is an error; existing expected output is replaced.
The two writes are sequential, as in the maintainer command's Rust host.
Its write confirmations are emitted even under `--json` or `--quiet`.

Load-only gate queries use the same authorized source loader as schema and
source queries. The loader retains canonical module, declaration, alias, and
import-edge name facts without checking function bodies. Gate projection reuses
sema's unsafe-obligation and policy writers, including the owning Package's
unsafe-path and expert ceilings; external writers are supplied as typed gate
entries. Diagnostics retain the loaded physical source path. A source that
loads successfully can therefore be inspected even when a callable body would
fail a semantic compile.

## Maintainer workflows

`jet self devtools reduce <file> --code <CODE>` minimizes line chunks while
the compiler still emits the requested error or lint. Without `--code`, the
oracle requires frontend acceptance and rejection of the generated Rust by
the hidden backend's metadata-only probe. An unavailable backend is not a
reproduction. The sibling output is `<stem>.reduced.<extension>`; files
without an extension use `.jet`. Candidates are compiled from their actual
temporary files, never by treating diagnostic display text as source input.

`jet self devtools ice-report <file>` writes the original source, generated
Rust, backend stderr, and both tool versions into a timestamped
`.jet/ice-report/` directory in the source project. Frontend errors prevent
publication. Backend acceptance is reported as a note but still permits a
bundle. Files are published sequentially, so an I/O failure can leave a
partial bundle.

`jet self devtools bless [target...] --dry-run` lists the selected snapshot
test commands without launching them or requiring a reason. A real sweep
requires at least eight UTF-8 bytes in the trimmed `UPDATE_EXPECT_REASON`;
it sets `UPDATE_EXPECT=1` for each child and inherits its standard streams.
Requested order and duplicate targets are preserved, and every selected
target is attempted even if an earlier one fails. These maintainer commands
own their raw arguments: `reduce` accepts the two-word `--code` form, and
`bless` treats options other than `--dry-run` as target names.

`jet inspect gates [<entry.jet>] [--kind <kind>] [--scope <scope>]`
uses the canonical load-only gate query, not semantic body checking or emission.
Without an explicit file, normal project entry selection still applies. Kind
aliases follow the gate registry; scope matches the row's scope, domain, or
source label case-insensitively (`all` selects every row). Filtering does not
hide loader, obligation, or package-policy diagnostics.

Human output includes provenance and operation proofs. Groups of at least
16 non-security gates collapse to a count; security groups always retain every
entry. JSON uses the shared `inspect.gates` status envelope, complete byte spans
and line/column locations, nullable external locations, and alphabetically
ordered kind counts. These are direct inspection reports, including under
`--quiet`. Trust rows project typed records from the actual canonical trust
store; invocation choices and package/lock facts feed the same ledger before
its policy checks. No CLI source scan, manifest parser, or invented receipt
replaces those providers.

Acceleration inspection projects the optimizer's actual loop, vector, and
acceleration proof rows for the selected profile and machine layout. It does
not convert a source proof into a runtime measurement. Decision inspection
materializes the canonical ledger from those MIR producers and semantic
diagnostic rows. The same authenticated `jet.mir.decisions.v1` payload carries
the complete source/configuration/profile/target/implementation/artifact/run
identity and checked derivation references for live snapshots and replay;
readers reject altered identities, row IDs, derivations, and noncanonical bytes.

The CLI's `inspect.accel` status projects typed authenticated runtime receipt
rows separately from static vector and gate facts. Missing measurements stay
null; a source proof alone never marks an acceleration selected. The shared
D-ACCEL1 evaluator retains the release, parity, proof, measurement, and gain
checks in their canonical order. `inspect.decisions` projects numeric row IDs,
flat derivation references, per-kind counts, and the actual compiler journal's
canonical routes; transport-ledger JSON is not a CLI status envelope.

Runtime rows are read from authenticated current receipts through the held
RecordIndex. Selection rederives the canonical target input identity, filters
the current tool version, then chooses recorded sequence and receipt ID in
descending order. A valid newest receipt with no acceleration rows does not
reuse older measurements. Typed sections retain full-width integer values and
reject unknown fields, invalid variants, duplicate numeric row sequences,
reversed ranges, and values exceeding the compiled target's pointer width.

The shared WatchGraph starts from the compiler loader's actual physical input
paths, adds checked project sidecars, recursively referenced HTML/CSS/JS assets,
and literal runtime file/directory inputs. It also requires a certified
comptime-I/O read set. The current compiler does not expose that set: file-target
receipt discovery refuses with `#4736`, rather than presenting an approximate
input identity as authenticated. Directory-target collection uses the separate
canonical tree policy.

Explicit canonical-pass selections may report `canonical_route_gaps` alongside
actual `canonical_routes`: these producer-owned operation IDs say `no Jet pass`
only for absent Jet boundaries. They are availability rows, not execution
receipts. Ordinary invocations preserve the reference status schema and omit
the extension when there are no gaps.

Output inspection consumes sema's checked callable links and selection facts,
including the original source/semantic/machine names, typed parameters, inferred
failure contract, solved effects, and reference/declaration spans. The driver
selects a singular runnable Output (or an explicit address) before lowering;
inspection does not rename a function or reinterpret a text entry. Layout rows
come from the compiler's target-aware reflection engine, not the inspecting
host's pointer width. Inspection stops after canonical MIR production and does
not emit or execute the selected callable.

`inspect.output` reports the selected binding, callable identity, effective
failure contract and its source, solved required effects, and the canonical
application authority's granted/denied effects. An explicit address selects
the checked Output, not an arbitrary function with a matching name.

Canonical-route evidence is opt-in through the existing adapter evidence
environment (`JET_ADAPTER_MODE` and comma-separated `JET_ADAPTER_OPERATION_IDS`),
not an additional public compiler flag. The source driver receives these
choices as typed request options from its host and passes them to an
invocation-owned journal; ordinary compiles allocate no boundary snapshots.
Selected pass boundaries retain their actual input/output representations,
payloads and identities, source provenance, occurrence/order, and preservation
premises. Decision inspection consumes these captured records directly, never
reconstructing a route from decision rows, work counters, or a digest.
Requested Rust boundaries with no separate Jet producer have explicit
`no Jet pass` availability rows. These are not pass records and do not prove
whether the language meaning is folded into another Jet pass or is absent.

The source loader also exposes the physical manifests and module paths it
actually read, including failed-load diagnostics. This is not the full watched
input closure: embedded Core/generated paths are excluded, and its comptime
input coverage is explicitly uncertified until actual evaluator reads are
retained. Receipt consumers must not substitute all authorized sources for
that missing read set.
