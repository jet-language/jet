# Command-line interface

Jet's CLI is a command dispatcher, not a second language surface. This page is
for users choosing a command and for scripts that need the stable spelling of a
flag. The executable truth is the generated output of

```sh
target/debug/jet help
```

and the command table in
[`Compiler/JetCli/Source/Cli/Commands.jet`](../../../Compiler/JetCli/Source/Cli/Commands.jet).
The Rust host's copy in
[`crates/jet-cli/src/CLI.rs`](../../../crates/jet-cli/src/CLI.rs) is held
equal to it by `node Compiler/Bootstrap/check-command-table.mjs`. Run
`jet help <command>` for the complete options for one command; do not infer an
option from an older guide.

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
jet jobs [--graph|--status|--explain|--watch[=<on|off>]] [<name> [<job args>...]] List, inspect, watch, or run named project jobs
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

Development reload selection uses Foundation's shared [watch policy](../../../Compiler/JetFoundation/Source/Execution/WatchPolicy.jet).
The last recognized policy flag before the program-argument separator wins;
child arguments do not change the development session's reload behavior.

Retirement diagnostics use the [canonical Foundation retirement table](../../../Compiler/JetFoundation/Source/Syntax/Retirements.jet).
The flat foreign-effect roots retain their registered refusals and dotted
replacements, including `Dart` → `FFI.Dart` (E0119); a consumer must not delete
a retirement merely because it no longer advertises the old spelling.

For C and C++, the driver-specific options are `--project-root`, `--build-root`,
`-c`, `-o`, `-MMD`, `-MD`, `-MF`, `-MT`, reserved `--sysroot`, `-I`, `-D`, `-L`,
`-l`, `-std`, `-dumpmachine`, `-print-sysroot`, `-dumpversion`, `-v`, and
`--target`. The exact acceptance and forwarding behavior is documented in
[CC driver hosts](cc-driver.md).

## Results and optional history

A recorded comparison reports the finite result of its corpus, not a universal
proof. A recorded terminal execution outcome cannot become a pass merely
because retained samples compare equal. The [comparison adapter](../../../Compiler/JetCli/Source/Cli/Compile/TestComparisonCorpus.jet)
uses Foundation's comparison and evidence models rather than a separate verdict
engine.

Optional history failure must not change the command's result. Notices are
grouped by their corrective action so one unusable history store does not
produce a notice for every attempted write. The [history policy](../../../Compiler/JetCli/Source/Cli/Evidence/History.jet)
keeps this notice policy separate from the runtime worker and authenticated
receipt store. Evidence publication preserves immutable bytes, rejects symlink
destinations, and retains the index's saved status and recorded sequence on
repeat publication.

Receipt authority inputs must be hashed with the canonical bounded, held-file,
no-follow reader, including opened-handle and path-after identity checks; a
metadata check followed by an ordinary path read is not equivalent. Metadata
classification uses Core's `files.Stat`, not a parallel CLI metadata carrier.

## Jet-hosted `check` and `build`

The Jet-hosted CLI ([`Compiler/JetCli`](../../../Compiler/JetCli)) runs
`jet check <file>` and `jet build <file>` through the Jet compiler
(`jet_driver_compile`). It gives the compiler one authorized source root: the
nearest directory at or above the file that holds `package.jet`, or the file's
own directory for a loose file, with every `.jet` file under it (dot names,
`target`, `build`, `node_modules`, `bin`, and nested projects stay out) and the
generated C/C++ binding caches. Core bodies, metadata, and the canonical effect
source come from Foundation's single [`foundation_embedded_core()` accessor](../../../Compiler/JetFoundation/Source/Registry/EmbeddedCore.jet);
the CLI does not read a runtime Core directory or require a toolchain-root
environment variable. `node Compiler/Bootstrap/generate-core-sources.mjs`
generates Foundation's canonical embedded payload and Core-menu projection
from the frozen Rust tables. `node Tools/agent/check-core-surface-ledger.mjs
--write` runs that generator, and `--check` holds the generated payload current.

`jet build` writes `.jet/build/<stem>` in that source root. The default and
`--profile debug` builds use the Jet-native backend (`lir_lower_program` and a
static x86-64 executable); a program that backend cannot lower yet is refused
with E2104. `--release` (or `--profile release`) compiles the emitted Rust with
rustc and the release profile flags. Other build flags and profiles are
refused with E2104 until they are ported.
