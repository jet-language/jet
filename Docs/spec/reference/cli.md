# Command-line interface

Jet's CLI is a command dispatcher, not a second language surface. This page is
for users choosing a command and for scripts that need the stable spelling of a
flag. The executable truth is the generated output of

```sh
target/debug/jet help
```

and the command definitions in
[`crates/jet-cli/src/CLI.rs`](../../../crates/jet-cli/src/CLI.rs). Run
`jet help <command>` for the complete options for one command; do not infer an
option from an older guide.

## Root command index

The root index below mirrors `target/debug/jet help` in this checkout:

```text
jet registry <command>               Publish and manage packages
jet db [PATH] [--query SQL | --script PATH] Run a bounded SQL console and manage database migrations
jet inspect <command>                Explore code, builds, packages, and bindings
jet bind <name> [--shape automatic|native] [--freeze]
bind --policy automatic|frozen
bind <name> --update --preview
bind <name> --update --accept <candidate-digest> Resolve and record a checked foreign binding plan
jet project <command>                Inspect project files and modules
jet self <command>                   Manage the Jet installation and editor tools
jet diff [args]                      Compare two Jet programs by meaning
jet merge [args]                     Merge Jet programs without losing code structure
jet review <base.jet> <head.jet>     Review meaning, authority, and proof changes
jet run [<file.jet|dir>] [--no-prepare] [-- <args>] Run a program or project
jet jobs [--graph|--status|--explain|--watch[=<on|off>]] [<name> [<job args>...]] List, inspect, watch, or run named project jobs
jet generate <GeneratorJob> [--apply|--dry-run] [--entry <file.jet>] [--json] Run an explicit source generator with authority and a receipt
jet check [<file.jet|dir>]           check code without creating a binary
jet fill <file.jet[:line]>           Propose checked code for typed goals
jet test [<file.jet|dir>] [<filter>] [--watch] [--fresh] [--docs] [--where=<expr>] [--capture=<failed|all|none>] [--browser=<chromium,firefox,webkit>] [--browser-retries=<n>] [--browser-reporter=<text|json|html>] [--browser-ui] [--browser-visual] [--browser-trace] [--browser-scaffold=<name>] [--grade=generated] [--iterations=<n>] [--time=<s>] [--seed=<n>] [--corpus=<dir>] Run tests
jet test-compare <corpus.json> [--relation=<name>] [--json] Compare one recorded observation corpus against its relation
jet prove [args]                     Create a proof report for code and tests
jet status [<file.jet|dir>]          Show what the project has proved
jet build [<file.jet|dir>] | build --verify <receipt-id> Create a native executable
jet package --kind <desktop|game> --target <linux-appimage|macos-app|windows-msix> [--executable <path>|<source.jet>] [--output <path>] [--profile <dev|release|name>] [--phase <build,cook,stage,package,export,deploy,run>] [--backend <aot>] [--renderer <headless|raylib>] [--cook-mode <fast|reproducible|scripts-only>] [--export-preset <default|store|headless>] [--deploy-to <path>] [--crash-reporter <off|on|opt-in>] [--crash-consent <not-requested|granted|denied>] [--dry-run] [--explain] [--resume|--cancel] [--clean|--scripts-only] [--run-now] [--package <id>] [--name <name>] [--version <version>] [--icon <path>] [--icon-format <png|icns|ico|svg>] [--icon-size <pixels>] [--publisher <name>] [--description <text>] [--update-channel <channel>] [--update-url <url>] Create a desktop or game application bundle
jet flash --target <board.name> [--image <firmware.elf>] [--audit <target.json>] [--adapter <probe-rs|openocd|emulator>] Flash firmware to a target board
jet cc [options] <sources>           Compile and link C with the pinned Jetpack toolchain
jet c++ [options] <sources>          Compile and link C++ with the pinned Jetpack toolchain
jet dev [<file.jet|dir>] [--canvas|--app <function>] [--share <loopback|lan>] [--token <token>] [-- <args>] Watch and run a program; optionally open Canvas or a local app
jet learn [--check] [--watch|--watch=off] [--json] [--quiet] [--color[=<mode>]] Practice Jet with offline code exercises
jet try <plan.json>                  Speculatively apply a plan and re-check its claims
jet debug [<file.jet>] [--record=NAME|--replay=NAME] [--dap] [--raw-frames] Debug a program from Jet source
jet repl [<file.jet>] [--project <dir>] [--console] [--sandbox data] [--console-ttl <milliseconds>] [--allow=<RIGHTS>] [--deny=<RIGHTS>] Try Jet code interactively
jet notebook [args]                  Open a Jet notebook (.jetnb) or Jupyter adapter
jet import <language> <dir> [--dry-run|--update] Convert supported source code into editable Jet
jet new <name> [--template cli|ui|web|overrides] | new service|route|job|migration <name> [--path <path>] [--route <path>] [--model <name>] [--up|--sql <SQL>] [--down <SQL>] [--risk <note>] [--lock <shared|exclusive>] [--version <n>] [--preview|--apply|--remove] Create a Jet project or backend source scaffold
jet fmt [args]                       Format Jet and configured project files
jet fix <file.jet|dir>               Apply safe automatic fixes, including `fix memory`
jet audit [args]                     Inspect implicit copies, exercised memory witnesses, or dependencies
jet lint --a11y|--complexity|--cost <file.jet> Run optional code-quality checks
jet doc [--json|--check] [<file.jet|dir>] Generate reference documentation
jet explain <CODE|FACT> [file] | explain --cost <file.jet> | explain --reload <file.jet|dir> Explain a diagnostic code, build fact, generic-module value, or typed cost
jet env <command>                    Open the project development shell
jet shared-store <command>           Manage the optional shared package broker
jet cache <command>                  Manage the machine-wide artifact store
jet remote [args]                    Manage host-owned remote builders
jet trust [args]                     Review or change trusted authority
jet image [args]                     Build a declared container image
jet os <command>                     Manage Jetos machines and images
jet add [args]                       add and download a dependency
jet remove [args]                    remove a dependency
jet fetch [args]                     Download locked dependencies
jet search <query>                   Search the local package catalog
jet find [--effect <effect>] [--example <input -> output>] [<query>] [<file.jet|dir>] Find code by type, effect, or example
jet update [args]                    Update dependency or toolchain pins
jet init [args]                      Create package settings in this directory
jet split [args]                     Extract closed Package facts into Configs or members
jet Fold [args]                      Reverse a recorded Package source transition
jet gc <command>                     Show values moved into automatic memory management
jet clean [args]                     remove unused package-store data
jet emit [args]                      Print generated build output
jet eval <file.jet|expression>       Evaluate pure Jet and print the value (`--json` for JSON)
jet budget [args]                    check performance limits or update baselines
jet perf <command>                   Collect and inspect performance traces
jet fuzz <file.jet> [<test>]         Find failing inputs for property tests
jet version [args]                   Show the Jet version
jet help [<command>]                 Show command help
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

For C and C++, the driver-specific options are `--project-root`, `--build-root`,
`-c`, `-o`, `-MMD`, `-MD`, `-MF`, `-MT`, reserved `--sysroot`, `-I`, `-D`, `-L`,
`-l`, `-std`, `-dumpmachine`, `-print-sysroot`, `-dumpversion`, `-v`, and
`--target`. The exact acceptance and forwarding behavior is documented in
[CC driver hosts](cc-driver.md).
