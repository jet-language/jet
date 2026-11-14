# Command-line interface

Jet's CLI is a command dispatcher, not a second language surface. This page is
for users choosing a command and for scripts that need the stable spelling of a
flag. The executable truth is the generated output of

```sh
jet help
```

and the command table in
[`Compiler/JetFoundation/Source/Registry/CLICommands.jet`](../../../Compiler/JetFoundation/Source/Registry/CLICommands.jet).
It is the one command table: handlers, commands, nested actions, and flags.
The compiler aggregate includes that Foundation registry exactly once; the CLI
imports its command, handler, and flag rows rather than maintaining a second
Jet copy. The bootstrap inventory contract checks their sole authored owner.
Run `jet help <command>` for the complete options for one command; do not infer an
option from an older guide.

Help and version are human renderables, not machine records: `jet help --json`
and `jet version --json` succeed with no output. Diagnostic essays instead
use the `jet.status/v1` `explain` action under `--json`. C and C++ driver aliases own their raw
arguments (including `--help` and `--version`) before Jet parses global flags.

A command that fails under `--json` prints one `jet.status/v1` line on stdout
and nothing on stderr. Its `reports` hold the typed refusal: a missing file or
project, a usage error, an unbuilt command, or a missing or mismatched engine.
The compile and authority problems of `jet run --json` keep their status line
on stderr, so the program's own stdout stays clean. `jet new --json` prints
one line with action `new`: `new.path` is the folder and `new.files` the
files it wrote.

`jet emit --ast|--ir <file>` prints one compiler tier of a file:
`--ast` the parsed syntax tree (parsing only), and `--ir` the slot IR `jet build`
builds from the checked functions of the entry package (every checked function
for a program without an entry). `--ir` checks the file in Run mode: errors stop
it and visible lints go to stderr. `--fn NAME` keeps one function. Each tier is
one tree. The text form indents each child two spaces. `--json` writes the same
tree as the `emit` status, a `{kind, text, children}` node per line. A function
the IR builder cannot build is reported as E2104 with its name, key and source
span.

`jet self devtools new-ui <name>` writes `tests/ui/<name>.jet` and its
diagnostic snapshot using the same compiler and renderer as the UI harness.
Only the source-file collision is refused; an existing `.stderr` is replaced.
`jet self devtools check-fixture-paths`, run from the repo root, checks that
every `fixture:` directive in `tests/cases/**/*.case` names a directory beside
its case; templated words such as `{work}` exist only at run time and are
skipped. Both commands retain their direct confirmation output under `--json`
and `--quiet`; these flags do not hide generated artifacts. Missing paths are
listed in sorted directory order.

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

Write a named chore as `#Job deploy: fn(region: String) { … }`, then run
`jet deploy --region eu`. Jet flags come before the job name:
`jet --release deploy --region eu`. Every word after the name belongs to
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
`run: fn`. Evaluation uses the same selection and checks transitive purity
through sema's checked call graph before execution.

## Published schemas

`#PublishedSchema` records have a lockfile-style snapshot in
`.jet/cache/schema/<Type>.snapshot`. The canonical sema codec records the
format version, type, published version, optional `squashed_before` cutoff,
and declaration-order field names and types. Field reordering is not breaking.
Removing a field, changing its type, or adding a required field needs declared
migration intent; invalid migration operations also report E0910 before
codegen. A type change requires an inline converter or an `impl New.From(Old)`.

`inspect schema status` uses the same sema diff over authorized load-only
declarations, intentionally without a full build's populated trait registry.
The full-build check remains authoritative. Snapshot reads honor
`JET_SCHEMA_CACHE_DIR`; publication and squash writes always use the project's
canonical cache directory. Source-only queries retain the loader's original
program, physical path, source bytes, and optional selected entry Package
version; they do not substitute a semantic check or reparse a manifest in the
CLI. Squash uses that declared version (falling back to `0.0.0` only when it
is absent).
Snapshot enumeration keys each valid snapshot by its filename, even if its
embedded type label differs. Status lists valid snapshots in type-name order;
unreadable or malformed snapshot files are skipped by this advisory listing.

Inspection planes and schema actions resolve explicit and inline Packages through
the same checked entry resolver as compilation. When a root is genuinely absent,
the action retains its own hint; a stale-name lookup uses the shared E1226
teaching diagnostic. These absent-root hints and stale-name reports are direct,
uncolored stderr user errors, including under `--json`. An authoritative resolver
error, including a retired `pkg.jet` manifest, instead retains source diagnostic
origins and spans, the requested color, and the `diagnostics` machine channel.

## Executable command metadata

Application completion reads the canonical v8 `JetCommandSchema` carrier,
including input defaults, subcommands, and all job execution metadata. The
Foundation byte reader recognizes ELF (both classes and byte orders), PE,
Mach-O (thin and universal), and Wasm custom sections. It verifies the record
length, digest, version, and field bounds before publishing a schema; missing,
duplicate, unsupported-format, and malformed records remain distinct errors.
PE permits zero alignment padding but not another record or nonzero trailing
bytes. Every universal Mach-O slice must contain the same single record.

The compiler's completion reader performs one nonblocking open and then inspects
and reads that same handle; it never executes the application. Its private Core
member `jet_std_files_open_reader_nonblocking` uses the existing runtime symbol
and `FileReader`/`IOError` carriers. The terminal-context and waited-child facts
likewise retain the existing `jet_term_is_terminal_raw` and
`jet_process_exit_status_raw` runtime spellings, rather than reserved
double-underscore member aliases or alternate host APIs.
Checked-build DNS and pinned byte transport likewise keep the
`jet_http_build_resolve` (runtime) and `jet_http_build_get` (Jet source over
`core.net` and `core.net.tls`) spellings. Driver retains address
authorization, bounds, digest verification, and error classification; these
leaves do not supply a second public HTTP or generic host policy.
Compiler-backed inspection and fix planning declare the execution host's actual
`Exec`, `IO`, `FFI.C`, and `FFI.Asm` ceilings. Fix semantic checks also declare
`Net` for the checked build-input HTTP provider already admitted by the CLI
package; these ceilings do not change the package's authority grants.
The compiler assembly retains the pre-merge command boundary: unwired
post-merge `Jobs`, package bundle/flash ports, and `Supply` remain in the
repository but are explicitly excluded from its source inventory.
Other unlisted CLI sources still fail assembly.
The CLI maps a failed byte fetch into the driver's typed build-HTTP error before
examining response bytes; it never lets the provider's failure carrier escape
through that boundary.
Held store linking keeps a distinct `compiler_link_held` source bridge: it
extracts the private held-directory handle inside Core and calls the existing
`jet_files_link_reader_at_raw` leaf. Compiler callers cannot substitute a path
or bypass the directory authority, and the bridge does not change link/copy
fallback policy.
The held-link leaf carries `FS.Write`, not the broad `FS` root. Raw file
routes preserve their source wrappers' narrow read or write contract;
inspection of already-held path, scope, platform, and process-ID facts
does not acquire filesystem authority.

Remote builder records accept LF or CRLF line endings, removing only the single
carriage return immediately before a newline. Extra or interior control bytes
remain malformed record data, not whitespace to silently normalize.
The remote command handles typed bind/list/remove failures at its output boundary:
each unsuccessful transport yields the existing E2104 report and exit code 2.
Successful actions retain their existing human and machine output.

Remote builder records accept LF or CRLF line endings, removing only the single
carriage return immediately before a newline. Extra or interior control bytes
remain malformed record data, not whitespace to silently normalize.

File-size arithmetic is bounded by `JET_COMPILER_TARGET_USIZE_MAX`, a
compiler-private constant folded from the selected build target's pointer
layout. It is not inferred from the inspected executable's ISA and requires
no runtime pointer-width leaf. The explicit bound in the byte-reader API
also permits portable boundary fixtures. Completion candidates retain written
order, positional ordering, and duplicate visible job names; internal jobs do
not become argv-reachable.
## Root command index

Bare `jet`, with no command (global flags such as `--color=never` allowed),
opens the command picker on a terminal (stdin and stdout both a terminal) and
otherwise prints the command palette and exits 0. The palette lists every
canonical command under its category (Build & Run, Projects & Environments,
Packages, Jetos, Development Server, Reference), then the Error Codes row and
a pointer to `jet help`; it is static text and never opens `/dev/tty`. The
picker draws on stderr: the palette's categories, fuzzy results while typing,
a command's usage and flags (Tab), and a two-pane reference on the alternate
screen (F1); a typed diagnostic code shows its `jet explain` page. Enter picks
a command, Alt-Enter its example, Escape or `q` quits. A picked line is never
run: at a bare terminal the picker names it on stderr, and with
`JET_HELP_SHELL_PREFILL` set (the `jet env hook` widgets for bare `jet` and
Alt-?) it prints the line on stdout for the shell to put in its edit buffer.
`--json` always gets the palette. Only `jet repl` starts the REPL: a line session that checks each
input against the session so far, then builds and runs it after the earlier
statements; `:help` lists its meta-commands. Raw-terminal keys, `--console`,
`:rerun`, `:history` and `?name` docs for names the session did not define
report E2104 until they are built (#6195). `jet eval CODE` runs CODE as the
body of `run: fn`, as `jet -e CODE` does (below), and `jet eval --json <expression>`
runs one REPL turn and prints its value in the status envelope. `?` is not a command: `jet ?` gets the
unknown-command E2101.

The root index below mirrors `jet help` from this checkout's compiler. It lists
the most used commands first, in the order of each command row's
`frequency_rank` in the compiler's command registry; `jet help --sort az`
lists the same commands A to Z and `jet help --sort za` Z to A. `jet
Tools/cli-census/Census.jet --write` writes the index from the command rows,
and its `--check` (a landing ratchet) fails when the index differs from them
or the ranks don't number the commands 1 to N:

<!-- BEGIN GENERATED CLI COMMAND INDEX: jet Tools/cli-census/Census.jet --write -->
```text
jet run [-A] [<file.jet|dir>] [--no-prepare] [-- <args>] Run a program or project
jet build [<file.jet|dir>]           Create a native executable
jet check [<file.jet|dir>]           Check code without creating a binary
jet inspect <command>                Explore code, builds, packages, and bindings
jet test [<file.jet|dir>] [<filter>] [--profile=<name>] [--filter=<text>] [--shard=<i/n>] [--list] [--results=<file>] [-O0|-O1|-O2...] [--time-limit=<duration>] [--compiler=<jet>] [--capture=<failed|all|none>] [--docs] [-u] Run tests
jet fmt [args]                       Format Jet and configured project files
jet explain <CODE|FACT> [file]       Explain a diagnostic code, build fact, generic-module value, or typed cost
jet dev [<file.jet|dir>] [-- <args>] Watch and run a program
jet repl [<file.jet>] [--project <dir>] [--allow=<RIGHTS>] [--deny=<RIGHTS>] Try Jet code interactively
jet self <command>                   Manage the Jet installation and editor tools
jet fix <file.jet|dir>               Apply safe automatic fixes, including `fix memory`
jet os <command>                     Manage Jetos machines and images
jet prove [args]                     Create a proof report for code and tests
jet fetch [args]                     Download locked dependencies
jet debug [<file.jet>]               Debug a program from Jet source
jet new <name> [--template cli|ui|web|overrides] Create a Jet project
jet registry <command>               Publish and manage packages
jet perf <command>                   Collect and inspect performance traces
jet help [<command>] [--sort frequency|az|za] Show command help
jet budget [args]                    Check performance limits or update baselines
jet env <command>                    Open the project development shell
jet eval <file.jet|expression>       Evaluate pure Jet and print the value (`--json` for JSON)
jet review <base.jet> <head.jet>     Review meaning, authority, and proof changes
jet audit [args]                     Inspect implicit copies, exercised memory witnesses, or dependencies
jet import <language> <dir> [--dry-run|--update] Convert supported source code into editable Jet
jet update [args]                    Update dependency or toolchain pins
jet emit [args]                      Print generated build output
jet doc [--json|--check] [<file.jet|dir>] Generate reference documentation
jet init [args]                      Create package settings in this directory
jet add [args]                       Add and download a dependency
jet jobs [<name>]                    List named project jobs or show one
jet cc [options] <sources>           Compile and link C with the pinned Jetpack toolchain
jet bind <header.h> --pkg <lib>      Resolve and record a checked foreign binding plan
jet lint --a11y|--complexity <file.jet> Run optional code-quality checks
jet image [args]                     Build a declared container image
jet status [<file.jet|dir>]          Show what the project has proved
jet gc <command>                     Show values moved into automatic memory management
jet merge [args]                     Merge Jet programs without losing code structure
jet c++ [options] <sources>          Compile and link C++ with the pinned Jetpack toolchain
jet version [args]                   Show the Jet version
jet diff [args]                      Compare two Jet programs by meaning
jet fold [args]                      Reverse a recorded Package source transition
jet find [--effect <effect>] [--example <input -> output>] [<query>] [<file.jet|dir>] Find code by type, effect, or example
jet trust [args]                     Review or change trusted authority
jet clean [args]                     Remove unused package-store data
jet split [args]                     Extract closed Package facts into Configs or members
jet cache <command>                  Manage the machine-wide artifact store
jet project <command>                Inspect project files and modules
jet search <query>                   Search the local package catalog
jet crash < <report.txt>             Name the functions and lines of a release crash report
jet try <plan.json>                  Speculatively apply a plan and re-check its claims
jet remote [args]                    Manage host-owned remote builders
jet remove [args]                    Remove a dependency
jet test-compare <corpus.json> [--relation=<name>] [--json] Compare one recorded observation corpus against its relation
jet fill <file.jet[:line]>           Propose checked code for typed goals
```
<!-- END GENERATED CLI COMMAND INDEX -->

One table, the command model (`JET_CLI_FORMS` in
`Compiler/JetFoundation/Source/Registry/CLICommands.jet`), says which `jet`
forms this compiler has not built yet: a command (`db`), a group action
(`self lsp`), a flag of a command (`build --lib`), a flag value
(`build --target wasm.wasi`) or a project shape (`<workspace with a missing
member>`). A form no row names is built. Help, `jet self man` and the shell
completions leave every unbuilt form out; running one answers E2104, whose Fix
names the Tower card that owns its port. `jet help --all` prints the usage page
followed by the full not-built list with those cards.

`jet try` reports compiler-query and checked-index failures through its existing
typed output error boundary before publishing a speculative verdict. Disk and
text query branches each translate their own query failure before joining the
checked result; only a complete disk baseline retains the rename index and
original source ownership.

The `bind` shorthand accepts the explicit binding-plan form shown by help:
`bind <name> --update --preview` or
`bind <name> --update --accept <candidate-digest>`. Foreign-header bind-in
commands remain under `jet inspect bind`.

`jet crash < report.txt` reads the stderr of a crashed release program, whose
report gives addresses and a `Build <id> (names and lines: jet crash)` line,
and prints the report a dev build gives: the function, file, line and source
of each frame, from the side debug file the build saved under
`.jet/symbols/.build-id/` (searched from the current directory upward). Text
around the report passes through. A report without the build line is E2114;
no side file for its build is E2115. The command takes no arguments.

Normal `jet fix <file|dir>` consumes the same checked diagnostic edits as
editor code actions. Without `--all`, both the applicability and safety class
must permit an unattended formatting or behavior-preserving change. Exact
duplicate edits apply once; overlapping edits are deferred and rechecked for
at most sixteen rounds. Each round's edited file is checked again: when it
has more errors than the file had before `jet fix`, each edit is retried on
top of the ones kept, and an edit that adds an error is rolled back and
reported with that error. One fix may carry several edits, some in other
files of the program: they apply together or not at all. A fix whose edits
overlap each other, or that names a file `jet fix` can't read, is refused
whole and reported; a fix with one edit overlapping an edit already kept
waits whole for the next round. The other files' errors count in the recheck,
the dry-run diff shows every changed file, and one transaction publishes them
all. Editor code actions offer a fix only when all its edits are in the open
file. The receipt's `skipped` rows record every edit left
out (one needing review, one rolled back, or one refused) with its reason. A `#Deprecated`
`use:` replacement is offered as an edit only when it is a plain rename, a
declared function that takes the same call; otherwise L2001's Fix says to edit
the use by hand. `--dry-run` renders the diff without publishing. A retired
spelling's error carries no teaching text, but `jet fix` registers its
rewrite: when an error falls on the retired build-time word before a block,
`NAME: comptime { value }`, every such block in the file is written
`#run { … }` in one pass (a comment or string holding the text stays).
`--edition=2027` performs the registered JSON/CBOR migrations first, retaining
written propagation and making panic fallbacks idempotent. `--quiet` suppresses
the human status. `--json` prints one `jet.status/v1` line with action `fix`
instead: `fix.file`, `fix.dry_run`, `fix.applied` (the count), `fix.skipped`
(each edit left out, with its `title`, `reason`, `span` and `new_text`) and
`fix.log` (the replay log, or null when nothing was written); machine mode
does not create a fake fix receipt. Publication uses the held codemod transaction and its durable undo
journal. Rename provenance comes from uniquely matched checked before/after
definition facts; when those checks cannot establish a rename, the receipt
records the exact changed byte region instead.

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

Package-source snapshots hold no-follow directory authorities and copy into
exclusive private stages before computing their content hash. Consumers use
the staged bytes, not a later read of the mutable original. Source copying
preserves empty directories, refuses symbolic and multiply linked source
files even under ignored names, and omits dot-prefixed entries, `build`, and
`target` from the package tree. A published source-store entry is selected by
the full package name, version, and lock fingerprint; a concurrent publisher
must have the same content hash. Source-store entries are separate from the
machine-wide artifact store and canonical Hangar.

Fetch resolves the package graph before publishing `.jet/lock`. Package
fingerprints include sorted dependency fingerprints and the frozen public
function signatures; release headers do not change the capability component.
Registry candidates bind the immutable source hash, signed index, OCI evidence,
dependency-role metadata, source policy and advisory authorization before
Hangar ingest. A selective update retains unrelated machine and semantic
records. The machine graph and its resolution rationales are serialized in one
atomic lock publication, after foreign provider bindings have been verified.
Provider-fact maps project into the canonical ordered `Dyn.Object` pair carrier
with keys in byte order, preserving deterministic identity and serialized facts.
Locked fetch verifies exact source identities and uses the existing registry
checkpoint without downloading an index.

Git validates the complete ordered DNS answer set, rejects the whole set if
any address is non-public, and pins subprocess connections to the validated
address. Local Git sources and private cache stages are passed as held
directory descriptors. If the host cannot transfer those descriptors to a
child, fetch refuses; a pathname is not an equivalent authority.

Checked source transactions hold the project, destination parents, and
`.jet/codemods` directory. Destinations and staged files must be regular,
single-link files with the identities observed before publication. The
schema-2 journal stores exact before/after bytes and staged identities before
the first source rename. Recovery completes the replay log when every file
reached its after-state, otherwise restores the recorded before-states;
unrecorded bytes or substituted identities preserve the journal and refuse.
Fix logs retain checked rename intent when the structural compiler facts
establish it uniquely, otherwise the exact changed UTF-8 region is a rewrite.
Undo verifies each log byte hash and every current after-checkpoint before
publishing any reversal.

Receipt objects and their append-only section records are immutable,
content-authenticated files under `.jet/receipts`. The record-index JSONL
snapshot under `.jet/records` is reloaded while holding its directory lock
before each mutation and replaced atomically with mode `0600`. FIFO eviction
removes only unreferenced disposable groups; removed artifacts are cleaned up
after the new index is published, without following links. The index and the
`.jet/evidence` reports are per-machine cache: they publish by the same
exclusive stage and rename but are not flushed to disk, so a crash can lose
the latest entries but never expose a substituted file. An index that does not
decode reads as empty, and an evidence report whose bytes differ from its
content id is replaced by the next publication of that id. Proof artifacts,
replay captures, receipts, and source transactions stay flushed.

Build-node evidence stays immutable in the machine-wide artifact store. The
latest record key for each program is workspace state:
`.jet/last-run/<sha256(program)>`. This pointer uses the authenticated
`jet.last-run/v1` record codec and binds the program name before resolving
the content-addressed build record. Missing or malformed pointers are cache
misses, not execution evidence. Publication holds the workspace directories,
locks the last-run directory, flushes a staged file, renames it, and flushes
the directory; separate workspaces do not share latest-run selection.
The program identity is the invocation's authorized selected entry, made
absolute and canonical when possible, then relative to the canonical workspace
root only at a complete path-component prefix. A selected build function does
not change this identity; neither a package name nor a filename stem substitutes
for it. Project dependency trees publish from held source handles, with
hardlinks where supported; Linux/Android `EXDEV`, `EPERM`, and `EOPNOTSUPP`
permit a byte copy and Darwin uses byte copies. Other hardlink errors refuse
publication rather than selecting a pathname-based fallback. Existing or
concurrently published dependency trees must match the source content hash.
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

`review <base.jet> <head.jet>` compares two error-free checked sides. Meaning
aligns the checked definitions of the one semantic index by stable id,
compiler identity, then signature and module; a recorded rename or move
receipt for the compared files aligns what the facts alone cannot. Authority
compares each side's gate ledger, including the package and lock writers, and
its checked effect rows; a widened or changed authority blocks `reviewable`.
`--base-receipt` and `--receipt` compare proof receipts, and equal sampled
traces never count as universal agreement. `review --diagnostics` instead
compares checker diagnostics by occurrence identity; a side whose checker
stopped before its bodies leaves relations unknown, never resolved.
## Command groups

The generated help groups subcommands as follows:

- **Registry:** `publish`, `yank`, `keygen`, `key backup`, and `vendor`.
- **Db:** bounded SQL plus `migrate new`, `preview`, `status`, `target`,
  `apply`, `step`, `rollback`, and `resume`.
- **Inspect:** `types`, `rights`, `claims`, `shapes`, `decisions`,
  `structure`, `build`, `gates`, `graph`, `query build`, `explain-build`,
  `compiler`, `impact`, `provenance`, `digest`, `env`, `semindex`, `output`,
  `expand`, `schema`, `codemod`, `audit`, `sbom`, `bind`, `logs`, `info`,
  `outdated`, and `reserved`.
- **Project:** `parts`.
- **Self:** `toolchain`, `update`, `doctor`, `completions`, `man`, `devtools`,
  `lsp`, and authority-bound `exec`.
- **Environment and stores:** `env test|hook|sync|info|fetch-toolchain|vars`
  and `cache status|prune|limit`. `env vars` lists the compiler controls
  ([environment reference](environment.md#compiler-controls)).
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
projections from the checker that diagnoses E0921. Prepared `reflect().origin` reads
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
selected package's checked `build: fn` in one invocation-owned context. They
finish and validate its graph without executing actions or publishing generated
files. An absent root build entry reports the default pipeline (`build: null`
in the `inspect.build` status envelope), not an invented empty BuildPlan.
Target and action explanations use the checked declaration; file explanations
use its retained ownership, consumers, targets, and generated-source digest.
Unobserved action-cache status remains `unknown`.
`inspect build --times` instead reads the `.jet/build/times.json` record the
last `jet build` of the package wrote and shows how long each step and target
took and whether it was made or reused; compile caches are off by default, so
`jet build` makes every one (`--json`: the `BuildTimes` record under `times`);
with no record it fails with E2104.
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

The options below are intentionally named exactly as in `jet help`. Each
registered flag's `jet help` row opens with the commands that take it
(`With fmt/doc/split/fold:`); an unscoped row goes with every command. That
row is the one rule: a command that does not read its own flags refuses a
registered flag scoped to other commands with E2102 and exit 2 before it
does anything, naming the commands that take it (`jet init --check` writes
nothing and points at `jet fmt --check`), and `jet help <command>` lists
exactly the flags the command takes. A value written after a flag that takes
none is E2102 too, never ignored: a switch takes no `=value`
(`jet fmt --check=true`), and a true/false flag takes only `=true` or `=false`.

- `--target` selects a triple, `board.<name>`, or named database target;
  `--profile` selects `release`, `debug`, `hardened`, or a named bundle
  (`ci` is not built yet). `hardened` builds as `release` does and, like
  `policy: { harden: true }`, keeps the memory sentry: a raw access through
  an `#Unsafe` pointer outside its storage stops with its R08xx runtime
  fault and the gate's source location at every optimization level. A
  plain `release` build carries no sentry call.
- `--allow=RIGHTS` and `--deny=RIGHTS` set effect roots or leaves for
  `run`, `build`, `jobs`, `repl`, and `db`; `-A` (`--allow-all`) allows and
  acknowledges every right on `run` and `build`, so a run names none; `jet inspect gates` records these and the other
  invocation choices as gate rows; `--offline` reuses only cached
  signed toolchain and package data. A denial covers a host read made under
  another effect's row only as [host-reads.md](host-reads.md) classifies it:
  `--deny=FS.Read` does not stop the resolver's `/etc/hosts` read under `Net`,
  `core.sys` host facts under `Env`, `/proc/self` reads, or an `Exec` call's
  reads of its own child's `/proc` entry for a memory or open-file limit.
- `--locked` requires locked dependency and provenance facts, and refuses a
  dependency whose inferred effect row is wider than the one `.jet/lock`
  records (E3513); `--fixtures` selects an explicit fixture bundle.
- `--lib` makes `build` emit the native Library and C header; `--output` selects
  a named output; `--small` favors a smaller binary; `--checked-alloc` on
  `build`, `run` and `test` links the checked memory allocator, which stops at
  a bad free or a write after free.
- `--json`, `--quiet`, `--color`, and `--verbose` select output detail.
- `--watch`, `--docs`, `--coverage`, `--where`, and `--capture`
  control test or development execution; `--browser` selects browser engines.
- `--explain-partition` is the web-target build option that shows JavaScript or
  WebAssembly assignment.
- `--threads N` caps the compiler at N threads on `build`, `run`, `check`,
  `test`, `dev`, and `inspect` (D-JOBS1=A); without it the compiler admits
  threads automatically up to one per physical core it may run on (SMT
  siblings count once), fewer when other processes already keep its CPUs
  busy at start (`/proc/stat`'s `procs_running` count). It never limits the
  threads of the program jet
  builds or runs, every value gives the same diagnostics and output bytes,
  and `build --verbose` reports the cap it used. No
  environment variable sets it. `--jobs` and `-j` are usage errors (E2102)
  that point to `--threads`, because `jobs` names project jobs; `jet test
  --jobs=<n>` is the test setting that sizes the corpus pool.

For C and C++, the driver-specific options are `--project-root`, `--build-root`,
`-c`, `-o`, `-MMD`, `-MD`, `-MF`, `-MT`, reserved `--sysroot`, `-I`, `-D`, `-L`,
`-l`, `-std`, `-dumpmachine`, `-print-sysroot`, `-dumpversion`, `-v`, and
`--target`. The exact acceptance and forwarding behavior is documented in
[CC driver hosts](cc-driver.md).

## Remembered flags

A flag a command takes can be remembered, so it applies to every run of that
command without being typed. Its setting key is the command's dispatch form
with a dot for each space and the flag without its dashes, `-` written `_`:
`fmt.check`, `test.serial`, `registry.publish.no_sign`, `check.trace`. Two
files hold settings, both data only, in one `commands:` block of quoted keys:

```jet
commands: {
    "fmt.check": true,
    "check.trace": "out/trace.json",
    "run.deny": ["net"],
}
```

- The user settings file, `$XDG_CONFIG_HOME/jet/settings.jet` (else
  `~/.config/jet/settings.jet`), holds only `commands:`. A relative or empty
  `XDG_CONFIG_HOME` is ignored, as the XDG base directory specification says,
  so `~/.config` is used, and a program that sets its own relative
  `XDG_CONFIG_HOME` still runs under `jet`.
- The `commands:` block of the selected package's `package.jet`: the
  workspace member a typed `-p NAME` picks, else the nearest one at or above
  the working directory. The packages above it are not merged in.

A package's setting replaces the user's, and a flag typed on the command line
replaces both; a list replaces a list, never merging with it. A setting is
`true` or `false` for a switch, a string for a flag with a value, and a list
of strings for a flag that repeats. A relative path a compiler control takes
(`trace`, `keep_going_report`, `gc_trace`, `memory_ledger`) is resolved
against the directory of the file that declares it. A compiler control's
setting ranks below its environment variable, as `jet env vars --command=C`
shows. A remembered flag has the effect of the typed one: with `check.json`
set, a `jet check` that is refused or fails prints the same JSON status as
`jet check --json`. A missing file holds no settings. A malformed file, an
unknown key, a value of the wrong shape, or a flag that is typed each time (a
permission, a destructive flag, or one that names what one run works on, E2117)
is refused before the command does anything, each problem with its file and
line.

## Jet-hosted `check`, `build`, `run`, `test`, `new`, and `fmt`

The Jet-hosted CLI ([`Compiler/JetCli`](../../../Compiler/JetCli)) runs
`jet check <file>`, `jet build <file>`, `jet run <file>`, and
`jet test <file>` through the Jet
compiler (`jet_driver_compile`). It gives the compiler one authorized source
root: the nearest directory at or above the file that holds `package.jet`, or
the file's own directory for a loose file, with every `.jet` file under it (dot names,
`target`, `build`, `node_modules`, `bin`, and nested projects stay out) and the
generated C/C++ binding caches. The Core library bodies are read from the Jet
toolchain source tree, the directory that holds `Core/` and `Compiler/Prelude/`:
`JET_TOOLCHAIN_ROOT` when set, else the nearest such directory at or above the
input, so a checkout's own Core wins; with neither, `jet` compiles against the
Core sources built into it ([`CoreSources.jet`](../../../Compiler/JetCli/Source/Cli/CoreSources.jet),
see [Environment variables](environment.md)). A tree's compiler-private Core
parts are its own `Compiler/Prelude/CoreSourceParts.tsv` rows, so a part the
tree adds loads, checks and tests on a `jet` built before it. A Core file's `embed_bytes` and
`embed_file` read the named file beside it in the toolchain tree, or, with no
tree, the copy of it built into `jet`: the build embeds every file a Core
source embeds. The compiler build reads `Compiler/Prelude/Core.rows`, the Core
source rows, the Syntax files and the runtime units from the toolchain tree and
computes the Core-declaration tables from them
([`CoreDeclarations`](../../../Compiler/JetFoundation/Source/CoreDeclarations)):
the Core source rows, the call rows, the enum, payload and structure rows, and
the built-in Core sources. A declaration that disagrees with the sources stops
the build and names each problem. The canonical effect source and the effect
vocabulary are read the same way
([`EffectVocabulary.jet`](../../../Compiler/JetFoundation/Source/Registry/EffectVocabulary.jet)).
`jet inspect structure --core --json` prints the built tables: the bootstrap
steps, root types, modules, Core sources and each Core call record's effect
fields and Prelude symbol (`calls`). `jet Tools/agent/CoreSurfaceLedger.jet
--write` writes the Core surface ledger from them and `--check` holds it
current; `jet Tools/agent/RuntimeEffectRows.jet --check` compares the
jet.runtime interface with the `calls` records.

`jet check Core`, aimed at that toolchain's `Core/` directory, checks the whole
Core library: one compile imports every public Core module and checks every
function body, whether a program calls it or not. A lint in Core source is not
reported, nor one in a codec body the compiler generates for a Core type. Any
other `jet check` loads and checks only the Core declarations its program can
name; `JET_KEEP_GOING=1` loads and checks every declaration of each Core module
the program imports. `jet check --style` adds the opt-in style lints (L0535).
A declaration error such as E0358 stops a plain check after declarations;
under `JET_KEEP_GOING=1` the check goes on, and every package with no such
error keeps its declarations' facts (what copies, which traits a type meets),
so a clean dependency reports nothing beside the failing package's errors.

Source snapshots enumerate held no-follow directories and read each source or
foreign cache through one bounded opened file. File identities include the
opened object's device, inode, length, and exact modification timestamp;
directory identities use the stable object, not its changing contents. The
source walk rejects multiply linked files and changed parent/file identities,
and enforces the same entry, file, depth, and byte bounds as the bootstrap
authority. Retained convention listings preserve the provider's Unicode-name
fact, including a literal U+FFFD in a valid UTF-8 name. Load-only queries and
checked overlay compilation share this snapshot path.

The native bootstrap host also authorizes loose source roots without creating
a manifest. A missing `package.jet` preserves the compiler's source-root
namespace semantics; retired manifests, symlinks, and other authority errors
are not treated as missing. Existing manifests and source files retain their
checked handles and are revalidated before invoking the compiler.

`jet build` writes `.jet/build/<stem>` in that source root. Every profile uses
the Jet-native backend: the IR builder builds the functions `run: fn` reaches,
x86-64 selection compiles them to a relocatable object, and the in-process
linker binds it to the runtime pack shipped beside `jet` into a static x86-64
executable. The default build is O2 (`optimize: O2`); `-O1` is
O1 and `-O0` (or `--profile debug`) is O0. `-O0`, `-O1` and `-O2`
are the optimize level flags; two levels that
disagree, such as `-O1 -O2`, are refused with E2104, and a level
the project pins is refused with E2113. A program that backend cannot build
yet is refused with E2104.
Other build flags and profiles are refused with E2104 until they are ported.

`jet run <file>` (and the `jet <file>` shorthand) builds the same executable
with the same profile rules, then runs it on the caller's stdin, stdout, and
stderr and exits with its status (1 when a signal ended it). The program's
arguments are the words after the first standalone `--`, or else the
positional words after the file; under the shorthand every word after the
file is the program's (a `--` right after it only separates), so a
`#!/usr/bin/env jet` file runs as `./file.jet args`. Advisory lints stay hidden unless `--verbose`
or `-v` comes before `--`; a verbose run reports them before the program
starts, then one line naming the effects the program uses
(`jet run: effects used: Exec, IO, Time.Wait`), so `jet run --verbose -A`
shows what the blanket grant let through. The list follows every call and
every function the program takes as a value (a function stored in a list, each
implementation a trait-value call may run), and a C call lists `FFI.C`. `--json` reports the same problems as the standard `diagnostics` JSON
status envelope on stderr, leaving stdout to the program; it never changes the
exit status. Run flags other than `--release`, `--profile`, `--verbose`, `--quiet`,
`--json`, `--color`, `--threads`, `--allow`, `-A`, `--deny`, and the compiler
controls a run takes (`--keep-going`, `--trace`, `--opt-dump` and the others
the [environment reference](environment.md#compiler-controls) lists) are
refused with E2104, naming the card the command model gives them, until they are ported.

Jet tells a file's language by its extension. A `.jet` file, or one with no
extension, is Jet source. `jet run`, `jet build` and the `jet <file>`
shorthand also take a C file (`.c`) or a C++ file (`.cc`, `.cpp`, `.cxx`):
they compile and link it in one command of the C toolchain Jet programs
compile their own C with (`CC`/`AR` when set, else the C toolchain pack
`jetpack` provides), into `.jet/build/<stem>` in the nearest directory at or
above the file that holds `package.jet`, or the file's own directory. The
compile is O0 for `jet run` and O2 for `jet build`; `-O0|basic|full`
(or `-O0`, `-O1`, `-O2`) picks O0, O1 or O2, and `--release` is O2. A
run then runs the program on the caller's stdin, stdout and stderr with the
same arguments a Jet program gets, and exits as it exits. The C toolchain
links no C++ standard library. The compiler's report of a file it rejects
goes to stderr, and the command exits 1. Other Jet flags are refused with
E2104. A file with any other extension is refused with E2116, which names
the extensions the command takes: `jet check`, `jet doc` and `jet dev` take
`.jet` alone. The exception is a script whose first line is a `#!` line naming
jet (`#!/usr/bin/env jet`). It is Jet source whatever its name, since the
kernel runs it with jet.

`jet -e CODE [args...]` runs CODE as the body of `run: fn`: `jet -e 'print(1 + 2)'`
prints 3. A `;` between statements stands for the line break a file would
have (`jet -e 'name := 1; print(name)'`). When the last statement is an
expression, its value is shown, as `node -p` shows it: `jet -e '1 + 2'` prints
3, a final `if` with `->` branches or a final dispatch is the value
(`jet -e 'if 1 > 0 -> "yes" else -> "no"'` prints `yes`; the same `if`
before the last statement is an unused value, E0003), a String prints without
quotes, a List or an Iter prints one item per line (`jet -e '"a b".words()'`
prints `a` and `b`), and a unit value (`print(…)`, an `if` without `else`)
prints nothing more. A value that has no display is refused with E0112, as `print`
refuses it. CODE is checked as a file is, so a
misspelled name is refused before anything runs. The file that holds it is
written under the scripts directory (`eval/`, named by the content hash),
and the rest runs as `jet <that file> [args...]`, so the same code reruns the
same file. Flags that come before `-e` are the run's. `jet run -e CODE`
runs the same way, and so does `jet eval CODE [args...]`, whose flags between
`eval` and CODE are the run's (`jet eval -A 'input.to_list().len()' log.txt`).
A `-e` after the program (`jet run hello.jet -e …`) is
refused with E2104, and `jet dev -e` with E2102. As a REPL cell does, CODE
may declare: a line of it (between `;`s) that is a declaration alone, such as
a type, a function, an `impl` or a `use`, goes above `run: fn`, so
`jet -e 'S: struct { amount: Float }; input.csv<S>().map(fn(s) { s.amount }).sum()' sales.csv`
prints the column's sum. A binding such as `x: 1` stays in the body. A Core
module is read by its full name with no `use` line
(`jet -e 'core.files.exists("out.txt")'`): when CODE names `core.` and binds
no `core` of its own, its file opens with `use core`, which binds the Core
root as a namespace, as it does in a file.

CODE reads the program's input through plain Core names, which work the same
in a script: `input` is the lines of the files named after CODE, else of
standard input (`jet -e 'input.map(fn(l) { l.words()[1] })' log.txt`);
`input.csv<Row>()` and `input.json<Row>()` decode it into records;
`input.edit(change)` rewrites each named file in place
(`jet -e 'input.edit(fn(l) { l.replace("http:", "https:") })' *.md`). `jet help
input` lists every such name with an example, and `jet help run` points to it.

`jet run - [args...]` runs the program read from standard input
(`jet run - < hello.jet`, or a pipe). Its source is written the same way, under
`stdin/` named by its content hash, and runs as `jet run <that file> [args...]`.
It gets the authority decision the same program gets as a file. An E1803
refusal names it as the command did: its Fix runs `jet run --allow=… -` and
puts the `package` block at the top of the piped source, as there is no file
for `jet fix` to edit.

A script is a standalone program wherever it lives: its source snapshot is the
script alone, and the source walk of a package leaves every script out.

`jet run` and `jet <file.jet>` compile the program on every run, for scripts
and for any other file, and keep no program: a rerun of unchanged sources
compiles again and writes nothing under the scripts directory
(`$XDG_CACHE_HOME/jet/scripts`, else `~/.cache/jet/scripts`; only the `eval/`
and `stdin/` source files above are written there). Keeping a built program
([`ScriptCache.jet`](../../../Compiler/JetCli/Source/Cli/ScriptCache.jet)) is
off, and no flag, variable or setting turns it on.

A plain `jet run` compiles at the stock `optimize: O0`, so a run starts
quickly; `jet run -O2` (or `-O2`) runs an O2 program, and no run
builds at a level it was not asked for. The compiled program runs in place of
`jet run` with its process id, so a signal sent to that process reaches the
program.

`jet build`, `run`, `check`, `test`, and `emit` take `--trace` to watch the
compiler work: one line per event on stderr as it happens, with the elapsed
time, each phase's start and end (with its duration, item count, and resident
and peak memory), the item being processed (a module or, with
`--trace-detail=functions`, a function; that detail also writes one
`ir.function` event per function the IR builder receives, so a trace names
every body the compile lowers), throttled progress counts, and the
scaling counters. `--trace=FILE` writes the same events to FILE as Chrome
trace-event lines, flushed per event, in the `JET_TRACE_FILE` trace format
(`Tools/perf/jetc-trace/summarize.mjs FILE --watch`
follows a running compile; Perfetto loads the file); timestamps have
millisecond resolution. Each `%p` in FILE is the jet process's id, so every
jet that a traced process starts with the same variable writes its own file.
`--trace=off` traces nothing. `--trace` is a compiler
control: `JET_TRACE_FILE=FILE` is its environment form for the same five
commands and for `jet self devtools core-record` (`-` or `stderr` selects
the readable stream), and the flag takes
precedence over it, as for every control (`jet env vars` shows which one
won); `--phase-counters` also prints each counter as a `JET_PHASE_COUNTERS`
line on stderr. The phases are the CLI's
`sources` walk, the driver's (`load` and its `parse` waves, `register`,
`sema.check`, `comptime`, the landing of local comptime values
(`comptime.land.open`, `comptime.land.splice`, `comptime.land.undo`,
`comptime.land.carry` per round), `sema.finalize` (each body pass opened
in `sema.pass_open`, the Core demand's `sema.core_demand.seeds` and one
`sema.core_demand.name` per wave),
`comptime.bodies` (holding, per body pass, `comptime.body.pending` and the
`comptime.body.run`, `comptime.body.splice` and `comptime.body.recheck` spans,
whose detail names the bodies the pass evaluates and whose index is the
pass), `comptime.validate`, `lower`, `assemble`, `optimize`,
`driver.result` holding `emit`, `driver.release` as the compile's
intermediate state drops (for an executable, the graphs drop on a detached
task while `ir.build` runs, so the span holds only what stays), and under
`--keep-going` `keepgoing.final` and
`keepgoing.report`), the host's checks of the result (`cli.diagnostics`,
`cli.authority`), `jet build`'s `store.open` (the machine-wide artifact store,
which a cold store lays out first; opened only while saved work is reused) and
the native backend's (`native.sources`,
`native.rust_foreign`, `ir.build`, `ir.opt` (named by the build's optimize
level, such as `O2`), `ir.x64`, `ir.object`,
`native.foreign` (the wait for the program's C closure and the runtime's
bytes, which its own thread realizes and reads while the object builds; for
a closure only checked foreign declarations name, also the link's live
symbol collection and the realization of the providers it reaches),
`native.link`; one `native.build` around
them for a test harness). A package build (`jet build` of a `build: fn`)
also traces `build.plan`, the check of its build entry and the plan's query,
and one `build.step` per batch of its steps, named by the steps. A step's
own processes are not jet's, so the span's `proc_cpu_ns` is jet's alone;
with `--trace=FILE` each step runs with `JET_TRACE_FILE` naming `%p.json`
in its sandbox, and when its batch ends the trace of every jet process it
started (a `jet run` of a script and the jets that script runs, the
compiler build's `core-record` with its Core check, record write, read-back
and embed) is added to FILE as its own process, named by the step, on
FILE's clock. The readable stream shows only jet's own events. The
readable stream is written through `/dev/stderr`, so redirect stderr to a
file with `2>>`, or use `--trace=FILE`. A trace that cannot be written
stops; the compile does not.

Before `jet build` or `jet run` writes anything, the Jet host checks the
application authority ([`Authority.jet`](../../../Compiler/JetCli/Source/Cli/Authority.jet)).
The law is spec.md "Application authority" (D-AUTH-DEFAULT1): every effect is
allowed, a run names its effects on stderr before the program starts and
again when they change, written rows are ceilings, and `--deny` wins with
E1803. The host compares the checked entry's solved effect row against the
floor (`IO`, `Mem.Alloc`, `Exec.Args`), the entry package's `authority` (from
`package.jet` or the entry's leading inline `package: { … }` block, which the
Jet loader reads with the same field parser and masks before parsing), and the
`--allow=`/`--deny=` rows of the invocation, where `-A` acknowledges every
right. A right a `deny` row or `--deny` names is E1803, exit 1, before the
program starts, and the Fix names the row to drop. Every other right the
floor, the `allow` row, `--allow` or `-A` does not cover is named in one
stderr line, `note: tool.jet reads files and uses the network (-A hides
this)` ([`EffectNotice.jet`](../../../Compiler/JetCli/Source/Cli/EffectNotice.jet)):
`FFI` and every `Exec` right but `Exec.Args` and `Exec.Exit` say what they
can reach is not bounded by the list. When standard input is a terminal the
set last named is kept per canonical entry path under
`$XDG_STATE_HOME/jet/effects` (else `~/.local/state/jet/effects`), and an
unchanged set is not named again; otherwise every run names it. `--json`
reports the set as one `run.effects` status on stderr instead. A local run
asks no question.

`jet run` of a target that is not a local path runs remote code
([`RemoteRun.jet`](../../../Compiler/JetCli/Source/Cli/RemoteRun.jet)). A
word that exists as a file or directory, or as `<word>.jet`, is always local.
An `https://` URL names one Jet file; plain `http://` is refused with E1805.
A word of a letter, then letters, digits, `-` and `_`, optionally followed by
`#<version>` (`greet#1.4.2`, `greet#1.4`), names a registry package, resolved
as a script's `use greet#1.4` resolves (the Hangar's `remote/registry`
launcher's `.jet/inline-deps`, then `JET_INLINE_DEPS_FIXTURES`, then
`jet fetch --lock` through the signed index). A bare name takes the highest
version this machine already has and is refused when it has none. Jet
fetches the target, stores a URL's file under the Hangar's
`remote/<sha256>/` folder, pins its sha256 (a package's tree hash), and runs
the pinned copy as `jet run <path>` would, with the same flags. Nothing is
written to the working directory. One record per target under
`$XDG_STATE_HOME/jet/remote` (else `~/.local/state/jet/remote`) keeps the pin
and the effects the user accepted for it.

Before the program's first instruction, a remote run asks about every effect
outside the floor, `--allow` and `-A`. The fetched code's own `allow` row
acknowledges nothing here. On a terminal, stderr shows
`tool.jet from example.org (sha256 3f9a0c1d2e4b…), new to this machine`, then
`it reads the clock`, then `Run it? [y/N] `. In place of `new to this
machine` the first line says `changed since you said yes` when the pinned
bytes changed, or `now asks for more than you said yes to` for new effects.
Any answer but `y` or
`yes` prints `not run` and exits 1. A yes is kept with the pin, and the same
bytes with no new effects run silently after that. With no terminal, or
under `--json`, the run stops with E1804 instead. `-A` or `--allow=` rows
acknowledge the effects on the command.

`--sha256=<hex>` pins a URL's first fetch. Bytes with another sha256 are
refused with E1805, and so is a `--sha256` that differs from an existing pin.
On a local path or a package it is a usage error (E2104). `--locked` runs
only what this machine already pinned and refuses to fetch or re-resolve
(E1805).

`jet run --allow='FS.Read("<path>")'` grants `FS.Read` confined to that path: the
program may read the named file, or the named directory and everything
beneath it, and nothing else (several such rows grant each path; `-A`,
`--allow=FS` and `--allow=FS.Read` lift the confinement). The path is
relative to the working directory. The kernel enforces it (Linux Landlock,
[`Loader.jet`](../../../Compiler/JetBackend/Source/Image/Loader.jet)
`x64_confine_reads`) on the file an open reaches, so neither `..` nor a
symbolic link leads out, and processes the program starts are confined too.
Any other read fails with a `PermissionDenied` error whose cause names E1803
and the granted paths (`JET_READ_SCOPE`). A kernel that can't confine the
run refuses it instead of running it unconfined. A package's
`authority: { allow: [FS.Read("<path>")] }` row grants the same thing: its
runs read only those paths, an `--allow` row's paths join them, and
`--allow=FS.Read` lifts the confinement. A package's paths are relative to
the directory holding its manifest, wherever the run starts.

`jet doc --check [<file.jet|dir>]` exits 1 when a public API item has no `///`
summary (L2201). Command entries are not library API: a module-level `run: fn`,
`build: fn`, `test: fn` or `dev: fn` (a `#CLI` program's entry is its `run: fn`)
and a `#Job fn` are exempt, in a loose script and a package alike.

`jet test <file> [<filter>]` compiles the file in test mode: each named
`#Test` body is checked and lowered as a synthetic function (a property
test's ungeneratable parameter types are E0613, a file with no named test is
E0601), and the artifact is a TestExecutable whose harness owns `main`. The
non-release harness is lowered by the Jet-native backend and linked with the
matching packaged test runtime as `.jet/build/.test_<stem>.<pid>`; release
keeps the emitted harness path. Native property generation is refused. Both
paths preserve `JET_TEST_CAPTURE`, `JET_TEST_FILTER`, `JET_TEST_SHUFFLE_SEED`,
`JET_TEST_SERIAL`, `JET_TEST_TRACE_TIERS`, `JET_UPDATE_SNAPSHOTS`, and
`JET_PROJECT_ROOT`, and set `JET` to the running `jet`, so a test that runs
`jet` (or a script whose `#!` names it) runs the compiler under test, as a
profile's children do. Native outcomes use process-private framed record files,
never stdout, kept in memory: in the per-user runtime directory
(`XDG_RUNTIME_DIR`), else `/dev/shm`, and in `.jet/build` only on a host with
neither. A run removes its own harness and record files on every exit path;
one killed before that leaves them, and the next `jet test` removes every
`.test_<stem>.<pid>` file in both directories whose process is gone (where
`/proc` lists the live ones). A record file the run can't create, write or
read (a full runtime directory) is E2105 naming the file and the operating
system's reason. After a fatal trap only unreported tests rerun individually,
one child per core at a time (fewer when free memory can't hold 1 GiB per
child; one at a time under `--serial`), reported in test order. A test that
reaches `core.sys.set`, `core.sys.unset` or `core.sys.set_current_dir`
(`core.files.chdir`) never runs in the batch: it runs alone in a child of its
own the same way, so a change it makes to the environment or working
directory reaches no other test. If the batch died and no rerun fails, the
run fails with a `native test batch` record naming the tests the batch left
unreported (under `--serial`, the one it was running).
A filter selects each test whose name contains it, and every test of the
source file whose name without `.jet` is the filter: `jet test tests/guards
--filter syntax_reconciliation` runs the tests of
`tests/guards/syntax_reconciliation.jet`.
`jet check`, `jet build` and `jet run` check the root package's `#Test`
bodies too, without lowering them. So a body that `jet test` refuses, each of
them refuses with the same diagnostic.
Output checks run after joins in the same batch process. If that process
traps, each unrun check fails with E3015 rather than running against fresh
state in the final reporter. Its
captured output is shown under `--capture=all`, or for a failed run under the
default `--capture=failed`; the exit is 0 when the harness passed, else 1.
A source of the toolchain's Core library (`jet test Core/Encoding/Cbor.jet`)
tests as the Core unit the compiler loads, with Core's privileges: its
`__core_*` imports and `__` names resolve as in any Core compile, and only its
own #Tests and doctests run (its doctest claims are appended to that unit, so
they reach the module's own names); a runtime unit tests as itself. `jet test`
of a directory of the Core library (`jet test Core/Encoding`) runs
`jet test <file>` for each Core source under it that declares #Tests or
doctests, as a workspace runs its members.
`jet test Compiler --profile core` runs every Core #Test at optimize O0,
O1 and O2. `jet test Compiler --profile fuzz` runs the sema fuzz corpus
`tests/fuzz/sema` differentially. Each `invalid/<name>.<code>.jet` file must
fail `jet check`; `<code>` names the refusal it was written for. Every other
file must pass `jet check`. A `differential/` file that passes then runs at
optimize O0, O1 and O2: the O0 run exits 0 and prints exactly the
output `differential/manifest.tsv` pairs it with (an `exact-stdout` row), and
the O1 and O2 runs end with the O0 run's exit code and stdout. No check
or run may exit 101 (an internal compiler error), die by a signal or time out.
Its rows are `tests/fuzz/sema/<dir>/<file>` and
`tests/fuzz/sema/differential/<file> [<level>]`.
Each test setting (D-TEST-INVOKE1=A, the `JET_TEST_SETTINGS` table in
`Compiler/JetFoundation/Source/Registry/TestSettings.jet`) has three
spellings from one row: the flag `--shard=3/8`, the TestProfile field
`shard: 3/8` and the call `t.shard(3, 8)` on the `Test` value of test.jet's
`test: fn`. The layers are the selected test profile, then `test: fn`, then the
flags, each replacing the earlier; `jet test` runs `test: fn` once in a plan
step (the harness runs no test while `JET_TEST_PLAN` names the plan file)
before any test runs, and prints the settings it used and the layer each came
from whenever a profile or `test: fn` wrote one. The settings are
`docs`, `filter`, `capture`, `update_snapshots` (`-u`), `shuffle=<seed>`,
`serial`, `measure`, `trace_tiers`, `optimize` (`-O0`, `-O1`, `-O2`; one run per level
when it lists more than one), `time_limit=<duration>`, `opt_in`,
`shard=<i/n>` (the tests and corpus rows sorted, then every n-th from the
i-th), `list` (prints the selected tests and corpus rows, runs nothing, exits
0), `cases=<file>` (only the tests and corpus rows the file names, one name
or ID per line; blank and `#` lines skipped; beside #Tests, a `filter` or
`cases` that matches no corpus row selects none instead of failing, and a
corpus-only run refuses a name that is no row), `jobs=<n>` and `memory=<size>`
(the corpus pool's width and its memory bound, such as `16G`),
`allow_shrink=<ids>` (ui rows `-u` may bless with fewer diagnostics,
comma-separated), `diagnostic_coverage` (the ui suite's coverage entry instead
of its rows), `results=<file>` (one JSON document of every verdict, the
testkit's results shape, whose `jet` is the compiler that ran them; a run of
several levels or another `compiler` writes its children's verdicts there,
each level's rows at the tier `tests at optimize <level>`) and
`compiler=<jet>` (the jet that builds and runs the tests). `list` beside `-u`,
a shard beside `-u`, a shard outside `1 <= i <= n`, `jobs`, `memory`,
`allow_shrink` or `diagnostic_coverage` in a run with no corpus suite,
`diagnostic_coverage` in a profile with no ui suite, a
`cases` file that can't be read and a `results` file that can't be written are
refused with E2104 before any test runs, as is a `t.<setting>(…)` call that
`test: fn` makes after `t.run()`. A child run gets the decided settings in
`JET_TEST_SETTINGS` and decides nothing again; the parent keeps `compiler`,
`optimize`, `results` and the corpus-pool settings.
It also honors
`--show-default`, `--release`, `--profile`, `--verbose`, `--quiet`,
`--color`, `--threads` and `--json`.
Doctests (fenced ```` ```jet ```` blocks in `///` comments) run as claims of
the same harness, named `doctest <n> of <item> in <file>` (the `n`th doctest of
the documented item), and `--docs` runs them alone; a wrong `// =>` value
fails with E2901, and `-u` rewrites it instead (D-TEST4). `--measure` runs only
the claims with a `.measure` member, serially and under the release profile
unless `--profile` names one. It prints one record per claim, as a `measure
NAME:` line or, with `--json`, as one JSON object line with the same fields.
It also evaluates the claim's `.Test(name)` budgets against that record
(D-CLAIM-BENCH1). A directory target (or none, inside a package) tests the
package through its own `test: fn(t: Test)` when it declares one, else through
its stock entry. At a workspace root, each member
that can run `jet test` runs as its own child `jet test <member>` with the
same flags, one child per core, fewer while free memory can't hold 1 GiB for
each; each member's output is written whole, in member order, after a
`jet test: member <name>` line on stderr (muted by `--quiet`), and the exit is
the worst member's. The root's own `test.jet` holds only the code suites its
profiles run. `--profile <name>` naming a test profile of the package's
`package.jet` (`test: { quick: TestProfile{ suites: [tests], time_limit: 60s } }`),
or a bare package `jet test` when `defaults: { test: <name> }` names one, runs
that profile instead (`--show-default` runs the stock default): each run is a
child `jet test` with the other flags, the `tests` suite once, or once per
level `optimize: [O0, O2]` lists (a level flag such as `-O1` or a `t.optimize(…)`
call replaces the profile's levels). `time_limit:` stops each test that runs longer and fails it; each
test then runs alone, and with output checks the batch runs bounded by the
limit for each test and check. Each run's output follows a
`jet test: profile <name>, run <run>` line on stderr (muted by `--quiet`). Its
rows (each test and its outcome) are compared with the baseline
`.jet/test/<profile>/tests.rows` (`tests-<level>.rows` per level): a test the
baseline has passing that now fails or is gone is lost and fails the profile
with exit 1. A failing test fails the profile even when the baseline already
holds that failure: the baseline names what moved, never what may fail. A run
with no baseline, or under `-u`, records its rows as the baseline and keeps
its own exit. A code suite, declared in `test.jet` as `t.suite("perf", perf_rows)`
(D-TEST-SUITE2=A; the name is a string literal, E0635), runs as its own child
`jet test <package>/test.jet` that runs no #Test and calls only that suite's
function with a `TestRows`; each `pass(id)`, `fail(id, detail)` and
`skip(id, reason)` is one row, kept even if the suite then panics. The child
gets `JET`, the path of the `jet` running the profile, and
`JET_TEST_SUITE_ROOT`, the package directory as the profile named it. Under a
`filter` it also gets `JET_TEST_SUITE_FILTER`. Then `rows.selects(id)` holds
only for an id containing the filter, and `pass` and `skip` drop every other
id. A failing row is always kept. A suite asks `selects` before doing a row's
work: `jet test Compiler --profile packages --filter X` builds and runs only
the packages, scripts and engine-unit claims holding a row that matches X, and
`--profile core --filter X` only the Core files holding one. The rows
are compared with `.jet/test/<profile>/<suite>.rows` as a run's are, and each
failing row's detail is printed. A repeated id fails as `duplicate row id`, and
a suite that panics or exits non-zero adds the failing row `stopped` with its
panic message. When the profile names a `baseline:` ledger, a failing code-suite
row whose id it lists is KNOWN-FAILING with the ledger's card and reason and
doesn't fail the profile, and a listed row that passes fails it as
UNEXPECTED-PASS until its ledger row is removed. A profile naming a suite that
neither `suites:` nor `test.jet` declares, or a name both declare, is a
`package.jet` error (E1215) that `jet check` reports too.
A corpus suite (`suites: { ui: TestSuite{ ui_corpus: "../tests/ui" } }`,
or `golden_corpus:` / `cases_corpus:`, a directory relative to `package.jet`)
fails with E2105 when the directory is missing; otherwise the profile's corpus
suites run in this `jet` through the testkit pool (`Compiler/JetTestKit`),
which prints one row per case and tier (PASS, FAIL, KNOWN-FAILING,
UNEXPECTED-PASS, UNSUPPORTED, SKIP-HOST, OPT-IN, REFUSED, ERROR, TIMEOUT) and a
summary. Row IDs are relative to the deepest directory holding the package and
every corpus. The `jobs`, `memory`, `filter`, `cases`, `shard`, `opt_in`,
`list`, `optimize` and `diagnostic_coverage` settings steer the pool;
`time_limit` bounds each step's process tree to that much CPU time on each
core, which the host's other load does not move, with a wall-clock hang
guard at ten times it; the `baseline:` ledger
(`ID<TAB>card<TAB>reason` rows) holds the accepted failures. `-u` blesses
every selected row's snapshots. A sharded or filtered run never writes a
baseline and compares only the baseline rows it reported; code suites run in shard 1.
A filtered run whose suites together report no row fails with ``the filter
`X` matches no test: 0 selected``.
`jet test Compiler --profile ui` (also `golden`, `cases`, `corpora`, `full`) runs
the repository's corpora; a bare `jet test Compiler` runs `quick`. A bare
`--shuffle` and the settings not ported yet (`where`, `coverage`, `grade`,
`iterations`, `time`, `seed`, `corpus`, `watch`, `record`, the `browser_*`
rows) are refused in all three spellings. The evidence report and the package claims
floor are not ported yet: the outcome is the harness exit (the outcome for a
package without a claims floor). `jet test` keeps no results between runs:
every run runs its tests, and nothing is read from or written to
`.jet/test-results/`. Reusing a stored result is off, and no flag, variable
or setting turns it on.

`jet new <name> [--template cli|ui|web|overrides] [--annotated]` writes a
project: `package.jet`
with the template's `authority: { allow: [...] }` row, `.jet/lock` with the running toolchain
pin and the build stamp (the git revision when the folder is inside a
repository), the template's `run.jet` (the `jet run` home), the three
comment-only command homes `build.jet`, `dev.jet` and `test.jet` for
`overrides`, and `.gitignore`, then the status lines on stderr
(muted by `--quiet`). A bare `jet new` prints the command help with exit 2.
The backend scaffolds (`jet new service|route|job|migration`) are refused with
E2104 until they are ported.

`jet init [<script.jet>]` writes `package.jet` in the working directory,
named after the directory (`app` when it has no name) and pinned with
`jet: <channel>` to the running toolchain; an existing `package.jet` is E1252
and is never overwritten. With a script, each inline `use pkg#version` ref in it
is added to the new manifest's `deps` block, and `lifted N inline
dependencies into package.jet` precedes `created package.jet (pinned jet
<channel>)` on stderr (muted by `--quiet` and `--json`).

`jet update jet [<channel>]` re-resolves the project's toolchain pin, moved to
`channel` when given (else the manifest's `jet:` channel, else the running
toolchain's), and records the resolved version in `.jet/lock`; its status
line is on stderr (muted by `--quiet` and
`--json`), and a pin diagnostic is always on stderr with exit 1. Outside a
project it is a user error. `jet update` of dependencies is refused with E2104
until the package fetch is ported.

`jet fmt [paths…] [-] [-- paths…]` formats `.jet` sources with the Jet formatter
([`Compiler/JetParser/Source/Formatter`](../../../Compiler/JetParser/Source/Formatter)):
no paths walk the project root (the nearest `package.jet` directory, else the
working directory), a directory is walked sorted with the ignored directories
and snapshot trees skipped, every file is formatted before any is written, and
one read or parse failure reports every failure with exit 2 (exit 1 under
`--check`). `--check` lists the files that would change (root-relative,
sorted) and exits 1; `--diff` adds unified diffs and implies `--check`, so it
never writes a file either; `--dry-run` is both; every word after `--` is a
path, even one that starts with `-` (a missing one is a read failure); `-`
formats stdin to stdout under the `--stdin-path=` label; `--simplify` enables
the ratified rewrites. The retired-spelling status lines go to stderr unless
`--quiet`. `package.jet` goes through the typed Package formatter (the Jetpack
Package model, referenced into the Compiler unit from `Jetpack/PackageModel`):
an unreadable manifest stops the run with its E1215 manifest diagnostic.
Comments stay with what they sit by: one at the end of an entry's line stays
after that entry, an own-line one leads the entry below it, and one after a
record's last entry closes the record; a manifest the formatter cannot lay out
with its comments in order is left as written. Every other `.jet` file, a
`Config` record source included, goes through the Jet formatter.
`--changed` formats the `.jet` files git reports
changed against `HEAD` or staged (exit 2 outside a git worktree).
Ordinary-source formatting preserves declaration meaning: a raw
`#Import(c) name: fn(args) -> T = "symbol"` retains its symbol assignment
(also when the result type is omitted or other attributes are present), and
module bindings retain `:=` for mutable values and `:` for immutable ones.
These are not `--simplify` rewrites.
`--explicit-copies` checks each file, then writes `.copy()` at every implicit copy
site: the Safe `.copy()` edits sema gives the same source under
`#Policy(copies: .Explicit)`; calls that would not parse report E0122.
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

`jet audit copies [--json] [<entry.jet>]` (also `jet audit <file.jet>` and
`jet inspect audit copies`) lists the implicit copies of the entry's checked
package without writing anything. The entry is the named file, a directory's
stock entry, or the entry of the project around the working directory. The
sites are the ones `fmt --explicit-copies` writes: the Safe `.copy()` edits
sema gives every module of the entry's source root under
`#Policy(copies: .Explicit)`, each at its copied expression. The text form is
`implicit read-view copies: N` and then one
`  <path>:<line>:<col>  span=<start>..<end>  size=dynamic  kind=implicit` row
per site, sorted by path and then by offset. `--json` emits the `audit.copies` status
envelope with a `copies` array of `{path, line, column, span{start, end},
size, kind}`. A source that does not check reports its diagnostics on stderr
(the `diagnostics` envelope under `--json`) with exit 1. `jet audit memory`
is the memory-ledger audit; the dependency audit is refused with E2104
until it is ported.

`jet audit writes [--json] [<entry.jet>]` (also `jet inspect audit writes`)
lists every call of the entry's checked package that changes or takes a named
value, chosen and checked as `audit copies` chooses and checks it, without
writing anything. The sites are the Safe E0228 edits sema gives every module of
the entry's source root under `#Policy(writes: .Explicit)`: `&` before a value a
call changes (a `&` parameter or `&self` method), `&&` before one it takes. The
text form is `calls that change or take a named value: N` and then one
`  <path>:<line>:<col>  span=<start>..<end>  mark=<&|&&>  access=<write|take>`
row per site, sorted by path and then by offset. `--json` emits the
`audit.writes` status envelope with a `writes` array of `{path, line, column,
span{start, end}, mark, access}`.

`jet audit foreign [--json] [<entry.jet>]` (also `jet inspect audit foreign`)
reports the foreign code of an executable entry, chosen as `audit copies`
chooses it. It checks the entry and links its image in memory as `jet run`
does; the image is never written or run. The text form has one line per fact:
`foreign declarations:` (`none`, or a count followed by one
`  <path>:<line>  <language>  <name> -> <symbol>` row per `#Import(<lang>)` or
`#FFI(<lang>)` declaration), `FFI effects reached:`, `C libraries linked:`,
`dynamic loader:` (the ELF interpreter, or `none (static ELF, no needed
libraries)`), `needed libraries:` when the image names any, and
`host effects:`, the effects the entry asks of its host, which Jet's runtime
serves through system calls. The last line, `foreign code: none`, appears only
when the declarations, `FFI` effects, C libraries and loader needs are all
empty; otherwise it reads `foreign code: present`. `--json` emits the
`audit.foreign` status envelope with `declarations` (`{path, line, language,
name, symbol}`), `ffi_effects`, `c_libraries`, `loader` (`{interpreter,
dynamic, needed}`), `host_effects` and `foreign_free`. The exit code is 0
whenever the report runs; a source that does not check reports its
diagnostics as `audit copies` does, with exit 1.

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

The checked program's fact channel classifies view materialization, collection
copy-on-write, exact-integer spill, retained outcome construction, and
representation fallback. Inline-integer removal is a proof from typed
operand intervals, not an assumption about the runtime value. The cost query
first proves callable coverage using sema-owned roots, direct calls and stored
function references, including source methods and selected outputs. A reachable
callable without a complete checked body or specialization refuses with E2104;
it never produces a clean-cost claim. The collector retains nested loop depth
through control flow and closure bodies. `lint --cost` merges and deduplicates
hidden-cost lint rows (card #6652), omits optimizer-proven removals, and reports retained costs
inside loops. View-materialization lints remain sema-owned. Findings exit 1
on stderr (diagnostic JSON Lines under `--json`); clean invocations exit 0.
Bare `inspect explain-build <file>` selects the authorized invocation entry
under its authenticated workspace root, then reads the authenticated last-run
record. It always reports the planned steps the compiler knows: the JSON
document `jet.explain-build/v1` carries a `planned` list of `{kind, key,
subject, inputs}` beside `nodes`, the recorded steps with `duration_ms` and
`why_ran`. A program with no record exits 0 with `nodes: []`; the text form
prints `no build recorded for <program> yet; planned steps:` and one
`kind key subject inputs` row per planned step, with no time or reason. With a
record the text form prints the recorded rows. A record for another program
is refused with E2104.

Checked build queries use the Driver's invocation-owned build scope and real
build evaluator; the CLI does not reconstruct a graph from source spellings.
Build input reads pin the root and every parent directory without following
links, reject nonregular or multiply linked inputs, and bound reads through
one opened file. Parent entries and the file's identity, length, and timestamps
are checked before and after the read. File-URL input reads retain the
reference's final-link refusal and same-open-file bounds.

`inspect structure` presents the compiler's published name facts and the
structure slice of its gate ledger; it does not infer missing facts by scanning
source text. Frontend failures can publish a structure report before the
diagnostic report, so the machine stream can contain a successful envelope
followed by a failed one. `--core` presents the canonical bootstrap order,
root types, and module/type export declarations of the toolchain's Core
without compiling anything, so it needs no target or project.
The Jet semantic pass publishes unused-import, private-function, and
package-export liveness verdicts in source/span/code/name order, with the same
`unreachable` status and collector detail as the diagnostic. Resolved user and
Core lifecycle markers publish the consumer's site, package-edition status,
and replacement detail. Once checked modules exist, failed Doc compilations
retain these facts as well as the earlier resolved import edges.
The structure rows it lists are the `Structure.*` rows of the registration fact
table (`jet explain` shows each one); a test fails when a structure fact kind
and those rows disagree.

Offline syntax explanations read `SyntaxDictionaryData.jet`, a Jet-owned
dictionary of keywords, markers, types, built-ins, operators, and sigils.
Its token rows are every spelling of `JET_HIGHLIGHT_TOKENS`, the syntax
registry's classification of what a user types, under the kind its highlight
class gives it, and its marker rows every marker of `Compiler/Prelude/Markers.jet`
and `Compiler/Prelude/Traits.jet`; `Tools/agent/SyntaxDictionaryRows.jet` and
`Tools/agent/OwnedTraitRows.jet` generate those rows, whose `--check` fails when
they drift, and only their meanings are written by hand, on the row. A generated
row with no written meaning explains itself plainly (``The marker `#Unsafe`.``).
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

`jet explain` takes a diagnostic as its code (`E0001`), its full name
(`jet.StrayCharacter`), or its bare name (`StrayCharacter`), and all three print
the same registry entry. A bare name that is not a registered diagnostic falls
through to the other lookups.

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

`jet explain effects [<file>[:<line>]] [--caps] [--json]` checks the program
and prints every effect cap in force at one site, nearest first. A file
inside a package is checked under the package's entry, so the caps written
where its module is declared hold; a `#!` script is checked on its own. The
first line names the site and its function, as its source spells it (a
function of an inline module or a generic module instance reads as
`counter.bump`); then comes one line per rung:
the `#FX(…)` blocks and lambdas that hold the line (`block`, `function`), the
enclosing function's `-[…]>` row, a `comptime` rung for the Mem-only rule,
the cap of the trait a method implements (`trait`), the type rung (`type`),
the module rungs (`module`: each inline module cap, a generic module's cap
on each function of its instances, the file-head cap, and
the caps where the file's module is declared), the package `authority` rows
(`package`), and `run` (everything else allowed; `--deny` wins). A type or
module rung that writes nothing prints `(none written)`; each written rung names the
`file:line` that writes it. The last line, `reaches:`, lists each effect the
site reaches, deny-only ones such as `Panic` and `Mem` included, with the
first call that reaches it and where the site makes
that call. No site means the entry `jet run` picks; a file alone means its
`run` function and module. A line past the end is a usage error, and `:0`
gets the `jet explain marker` refusal. `--caps` lists every file-head,
module, type and trait cap written under a file, a directory, or (with no
target) the project root, as `file:line  #FX(…)  module pricing` (`file`,
`type Receipt`, `trait Shape`); a generic module's cap is listed under the
template's name. A program that fails its check still gets
its ladder on stdout, under the errors on stderr. `--json` prints one
`jet.status/v1` line with action `explain.effects`: `site`, `rungs` (`rung`,
`written`, `text`, `site`) and `reaches` (`effect`, `chain`, `site`), or
`scope` and `caps` (`file`, `line`, `cap`, `kind`, `name`). Every name it
prints, a `chain` step among them, is the source's, never a
compiler-generated one. This is the "`jet.report/v1` envelope" that
D-JET-EXPLAIN-EFFECTS1 names: the machine-output contract of
D-REPORT-MACHINE1 and D-REPORT-LAW1, under which a command result is one
`jet.status/v1` record and every report inside it is a `jet.report/v3`
object.

`jet self devtools new-example <area>/<name>` writes `Examples/<area>/<name>.jet`
and its expected output `Examples/<area>/<name>.out` beside it.
Existing example source is an error; existing expected output is replaced.
The two writes are sequential.
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
the compiler still emits the requested error or lint; `--code` is required. The sibling output is `<stem>.reduced.<extension>`; files
without an extension use `.jet`. Candidates are compiled from their actual
temporary files, never by treating diagnostic display text as source input.

`jet self devtools ice-report <file>` writes the original source and the
compiler version into a timestamped `.jet/ice-report/` directory in the
source project. Frontend errors prevent publication. Files are published
sequentially, so an I/O failure can leave a partial bundle. These maintainer
commands own their raw arguments: `reduce` accepts the two-word `--code` form.

`jet self devtools core-record --check` checks Core and the Prelude alone,
the closed program the precompiled Core record is written from, with every
Core body checked, and prints its module, function, constant, type and impl
counts, or the diagnostics that keep Core from checking clean. `--write <file>`
also writes the `jet.core/v15` record of that check to the file and prints one
line per record section at once: its element count, the bytes its elements
take and the bytes of its largest element. It then reads the file back and
encodes every row again, which must give the tree it was read from. The record
leaves out what a load gets back without it: a module's source text when the
jet embeds that Core source, the bodies of an export's declaration, in its
registration row and its checked module row alike (a check
reads another module's export for its header), the lookup columns of a
body's symbol-key and type tables, which follow from their rows, the ids
of a body's expression-type rows, which are its nodes', and the body's key in
each of its name references and call facts, which is its packet's. It writes
each body node's id as its step from the node before, and each node's and name
reference's span as an offset from the body's start and a length. It writes
each symbol key as its parts (kind, module, owner and name), with an owner
that is an earlier key of the same range named by that key's row. Each tree
object is its shape and its values' payloads, the shape naming each value's
tag. Each Core module's Core usage (the Core names its bodies reach) is a row
of its own, so a compile that takes the record walks no Core body for it. So
are its taint crossings: for each parameter of each Core function, the path on
which its body steers on a `timing: .Fixed` value passed there, so a compile
that takes the record walks no Core body to check a call that carries such a
value into Core. So are its effect facts: each Core function's `#FX(…)`
regions, loop spans, `assume_deterministic` spans and pending
`#Kernel(.parallel)` obligations, so a compile that takes the record walks no
Core body for the effect check either. So is its reach: what its source
spells, so a compile that takes the record loads the Core modules a module
imports or names only once reached code of it spells them, as a compile of
Core's sources does.
`--embed <image> <runtime-dir> <out> <identity>` writes the executable
`image` to `out` with the record added as its `jet.core` part and, in the
same pass, each runtime file of `runtime-dir` (the key, export and thread
lists, the prelinked image, the pack, the Jet-only object, the web partition
and its receipt, and `IDENTITY`, the identity of the jet that lowered the
runtime) as its `jet.runtime/<file>` part, and last its own identity as its
`jet.identity` part, which it also writes to `identity`. A jet's identity is
`jet@<release>#<digest>`, the digest the sha256 of the jet's bytes with its
`jet.identity` part's bytes zeroed: the sha256 of the file for a jet without
that part. The compiler build runs it with the `jet-bare` it just linked and
the `jet-runtime/` folder its `runtime` step built with that `jet-bare`, so
the `jet` it ships reads no runtime or identity file: it maps its runtime
image from its own executable and lends the other parts from its mapped
image. Only `jet-bare`, which carries no part, reads `jet-runtime/` and the
`IDENTITY` file beside it; a jet that carries the record but lacks a runtime
or identity part stops with an internal error. The record names the compiler
`JET_COMPILER_IDENTITY` names; unset, it names this jet by its identity part,
else by the sha256 of its own executable, never by an `IDENTITY` file, and
the runtime's `IDENTITY` must name that same compiler. `--embedded` reads the
record part from the running jet's mapped image, with no file read, and
checks it as `--write` does.

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
The compiler assembly includes only the trust store's read path. Gate-aware
grant writers stay in Jetpack's existing command source alongside its selector
and JSON writer; granting still checks policy before any store read or write.

`jet inspect memory [<entry.jet>] [--profile <name>]` compiles the program
through the same native pipeline as `jet build` and prints, for each entry
point (the program entry and every kept function), the worst-case stack in
bytes and the deepest call path, for example `stack: 96 B via run -> step ->
leaf`. Each function's frame is its return address, saved frame pointer,
saved registers, locals, and the largest outgoing argument area. A call cycle
with a `#MaxDepth(n)` function on it counts that function's n allowed calls
and stays bounded; a cycle with none reports `stack: unbounded, recursion via
...` and asks for `#MaxDepth(n)`; a
call through a function value reports `stack: unbounded, a call through a
function value via ...`. Runtime frames are not
counted; they are named on a `runtime frames not counted:` line. Under the
stack line, `stack bound:` gives the bound sema proved for the entry by the
frame model `!Mem.Stack(above: N)` checks; the backend holds every frame a
stack denial reaches to it: `stack bound: <n> B via run (<n> B) -> step
(<n> B)`, each step with its own frame's bound, or `stack bound: unbounded,
<why> via <path>`. In an executable build sema seals each call through a
function value to the functions the program takes as values and the lambdas
it writes, with the same argument count, and each call through a trait value
to the implementations the program holds, and the step names them:
``apply (<n> B + <n> B lambda, calls through values sealed to `double`, a
lambda in run)``, where the lambda part is the largest lambda frame the call
may run. A library build leaves them open: `stack bound: unbounded, a call
through a function value in apply, whose targets a library build leaves open
via ...`. The program
entry also gets its worst-case heap, by the proof `!Mem.Alloc(above: N)`
checks: `heap: 8 B`
is the arena bytes every call path from it may reserve, and
`heap: unbounded, <why> via <path>` names what stops the bound (an
allocation with no proven size, a call in a loop with no bound, recursion,
or an open dispatch) and the call path to it. JSON uses the `inspect.memory`
status with one `entries` row per entry point (`stack_bytes` is null when
unbounded; `stack_bound` is null when sema stamped none; `heap` is null
except on the program entry). This is a different report from
`jet inspect audit memory`, which reads exercised memory witnesses.

`jet inspect choices [<entry.jet>]` lists the automatic choices the compiler
made for the checked program, by kind, each with the function it was made in
and its reason. A kind with no choice prints `<kind>: none`. Storage form: a
reactive handle (`Signal`, `Derived`, `Computed`) that crosses into a task, a
channel, or a parallel worker, or that is pinned `#Shared`, is kept in
synchronized storage, for example `storage form: main.jet:3 in run: pending:
Signal<Int> synchronized for task crossing`. `#Local` locks the one-thread
form; a crossing then fails the build with E1102, which names the crossing.
Inlining: for a program with a `run: fn`, each call site the optimizer inlined
as `jet build` optimizes it (the profile's level and the build option flags,
so `--inline=off` and `-O0` list none), at the call's line, with
why it was taken: its size within the level's callee limit and the caller's
remaining budget, its only call, or `marked #Inline(Always)`.
`#Inline(Always)` locks the callee inlined; a change that makes it recursive,
address-taken, or too large fails the build (E0917-E0919).
Collection: under `#Policy(gc)`, a binding whose returned heap graph needs
identity beyond its lexical owner is kept by the policy scope's collector, at
the function's line, for example ``collection: main.jet:10 in build: `first`
collected under the module `#Policy(gc)`: returned heap graph requires
identity beyond its lexical owner``; a binding that takes such a value from
its call holds collected storage too.
Copies: each copy the compiler adds that the source does not write, at the
function's line. A binding moved by its plain name and read again later is
shared, for example ``copies: main.jet:4 in run: `items` shared: a later read
uses it after this move, so the move copies it``. A borrowed or still-used
value passed to a variadic list or `debug` is copied into it.
`#Policy(copies: .Explicit)` locks the share out: the move stays a move and
the later read fails the build (E0121).
Placement: in a program that opts in to the web partition (a `#Target` or
`#Export(wasm)` marker, or a `#Target` ceiling), the JS or Wasm bucket of each
function and why, at its line, for example ``placement: app.jet:4 in render:
JS: uses the `Browser` effect``. The reasons, in order of precedence, are a
pin, the `Browser` effect, the module's ceiling, a `Stream` result, and the
Wasm default. After those, a function can also move to JS to follow a JS
caller or callee. `#Target(JS|Wasm)` locks the bucket: a Wasm pin that uses
`Browser` fails the build (E-WEB-TARGET-BROWSER), and so does a call across
the buckets (E-WEB-CROSS-PARTITION). A row whose bucket the partition chose
names the pin that would lock it, for example ``placement: app.jet:8 in count:
JS: called by `view`, which runs in JS; pin it with `#Target(JS)` ``.
Exposure: each method an endpoint marker (`#App.SignedIn`, `#App.Public`) or
a route marker makes an endpoint, at its line, under the marker as written,
for example ``exposure: app.jet:12 in order: endpoint under
`#Server.Get("/orders/:id")`: the marker serves it; without it the method is
no endpoint``. A method with no row is no endpoint.
Context: the task context fields and keys each function's own body reads
(`context.<field>`, `context.get(key)`), at its line, in the order it first
reads them, for example ``context: main.jet:3 in tick: reads `deadline`,
`thread` ``. `context.scratch` is not listed: it names the arena values are
made in. Reading the context is no effect, so the effect row never shows it.
In the editor (`jet self lsp`), hovering on a line shows the choices made at
that line, each as `<kind>: <choice>`. Inlining is shown only by `jet inspect
choices`, because an editor check does not optimize.
JSON uses the `inspect.choices` status with `kinds` and one `choices` row
(kind, file, line, function, choice) per choice, in file and line order, and
in the order of `kinds` within a line.

`jet inspect target [machine] [--json]` reads only the toolchain's machine
table, never a project. With no name it lists every declared target machine
with its triple, environment, and the tiers it runs at; `--json` prints one
`jet.status/v1` line with action `inspect.target` whose `targets` array holds
`name`, `triple`, `environment` and `execution` (`aot`, `dev`) per machine.
With a name it shows that machine's target dossier (D-TARGET-AUDIT1). The text
form names the machine, its triple, environment and provider identity, whether
it runs ahead-of-time only or under `jet dev` as well, and the capabilities it
provides and lacks. `--json` prints one `inspect.target` line whose `target`
object holds `name`, `triple`, `environment`, `provider_identity`, every policy
(`linker`, `allocator`, `panic`, `memory`, `mmio_capability`, the four time
sources, `entropy`, `scheduler`, `byte_sink`, `startup`), `provider_facts`,
`hardware`, `programmers`, `execution` (`aot`, `dev`) and `audit`. A machine
whose audit policy has no dossier lens has no dossier, and an unknown name is
refused with the list of declared machines.

Build graph inspection (`inspect build`, `inspect graph`, and `inspect query
build`) shares the driver's checked plan projection. Its `files` and
`affected_files` fields both list the static file universe; this is distinct
from `core.build.receipt_diff`'s change-impact closure. Without an execution
record, action cache status remains `unknown`. Build vector rows retain the
checked program's declaration order rather than acceleration inspection's
key-sorted function view.

Compiler check/compile/link node identities come from the final checked bundle,
including selected generated modules, and use the shared Core build-key recipe.
Source names are normalized relative to the selected source root before hashing;
compile nodes depend on check identities, and link nodes on compile identities.
Selected outputs are sorted and deduplicated; an unnamed output uses the actual
entry file's stem. These static graph identities contain no measured duration or
rebuild reason. Only actual build action observations can populate a BuildRecord.
The native compile observation retains its actual object handle separately from
the graph key: target, profile, and runtime-pack identity are not encoded in that
static key, so it cannot serve as a native artifact cache identity. A compile
observation never stamps an unexecuted check or link action.

Decision inspection builds the checked program into the slot IR from its
`run: fn` and runs the IR optimizer at the level `jet build` uses: O2, or O1
under `-O1`, or O0 under `-O0` or the `debug`
profile. It reports what the
passes changed (instructions before and after, inlined and direct calls, folds,
fused compares, short circuits branched, coalesced copies, removed dead and unreachable code, scalar
replaced aggregates) and names each function the IR builder could not build.
At O2 it adds one row per sum, count or fill Loop the `reduce` pass handed to
an asm leaf or declined (`<form> reduction <function>: vectorized` or
`declined (<why>)`), one row per index a versioned Loop writes
(`stepped index <function>: guarded before the Loop`, or `declined (<why>)`
when that index is tested at the head of every pass), and one row per factor
of a versioned Loop's Mul that is guarded or that the Loop writes
(`factor <function>: guarded to 31 bits before the Loop`, so that the product
needs no overflow test, or `declined (<why>)`).
It also lists, as built before any pass, each counted loop sema's loop proof
covers (`loop <function> <file>:<line>`): the proved extent and element type,
the aliasing, early-exit, effect and cross-iteration verdicts, how many list
accesses sema proved in bounds and how many of them the build lowered with no
bounds check. Each loop lowers one scalar round per index.
It lists each masked branch (`masked <function> <file>:<line>`). A masked
branch is a branch on a `timing: .Fixed` value whose arms sema proved total.
Both arms run, and a masked select keeps the chosen arm's results, so no jump
depends on the value.
Last it names the program's `main:`. A program gets `program-owned` when none
of its stop edges cleans up anything a process exit could observe. Each edge
only writes and frees memory (its drops' routines too), enters and leaves a
cleanup scope, or returns. Every runtime route it calls must also behave the
same with no process root. Those routes include exact `Int` arithmetic, except
shifts, and the generated `#CLI` parser's (argument reads, `Int` parsing,
refusals on stderr and `process.exit`). It then runs under a main of its own
with no stop points and no cleanup scopes, and its `process.exit` publishes no
stop. A stop writes the same
report and exits with the same code as under the root. Any other program
gets `process root:` and the first function, route or stop edge that needs
the root.

`inspect decisions --estimate` then lists each large raw value: every data
row of at least 64 KiB the optimized program's image holds, as
`large value "<preview>": <n> bytes stored, zstd estimate <m> bytes`. The
preview is the row's first 16 bytes, printable ASCII as itself and any other
byte, `"` and `\` as `.`. The stored size is the bytes the image holds for the
value; the estimate is the length `core.archive.zstd` compresses them to, one
frame per 64 MiB. The compression runs only under the flag.

Live inspection consumes only the runtime's private snapshot. The runtime writes
raw records: little-endian words, and texts as a byte length then the bytes.
The reader decodes and checks them; the running program does not encode.
The reader's no-follow read keeps one file descriptor open and checks the file
type, owner and private permissions. It reads at most 1 MiB.
The snapshot binds to the requested process by pid and by a publication age
under two seconds. On Linux it also binds by start time: a process that started
after the writer's clock start is a reused pid. On Linux the reader takes the
process CPU time from `/proc`. The record has a magic number, sections that
appear at most once, no unknown sections and no trailing bytes. Tasks and
values are limited to 4096 rows each, and the view shows at most 4096 channels.
Event observations
keep exactly fifteen payload-free fields, at most 256 records, strictly
increasing sequences, and the runtime's closed enums and unavailable-count sentinel.
The reader derives the id order and the effect and resource counters from the
records. Each Unicode control character in a text is shown as one space.
Text and JSON output share the same validated snapshot, including published
rendered values and sorted shape identities. No process memory is read.

The CLI's `inspect.decisions` status carries the optimizer report's counts as
fields, the unbuilt functions, the loop proofs (`loop_proofs`) and the masked
branches (`masked_branches`) as text rows, and the program's main (`main`) as
text. Under `--estimate` it adds the large-value rows as `large_values`.

The shared WatchGraph starts from the compiler loader's actual physical input
paths, adds checked project sidecars, recursively referenced HTML/CSS/JS assets,
and literal runtime file/directory inputs. It also requires a certified
comptime-I/O read set. The current compiler does not expose that set: file-target
receipt discovery refuses with `#4736`, rather than presenting an approximate
input identity as authenticated. Directory-target collection uses the separate
canonical tree policy.

Explicit canonical-pass selections report `canonical_route_gaps`: no Jet pass
records a canonical boundary, so every selected operation ID says `no Jet pass`.
They are availability rows, not execution receipts. Ordinary invocations omit
the extension.

Output inspection consumes sema's checked callable links and selection facts,
including the original source/semantic/machine names, typed parameters, inferred
failure contract, solved effects, and reference/declaration spans. The driver
selects a singular runnable Output (or an explicit address) before lowering;
inspection does not rename a function or reinterpret a text entry. Layout rows
come from the compiler's target-aware reflection engine, not the inspecting
host's pointer width. Inspection stops at the checked program and does
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

Checked build-input HTTP fetches use a build-only transport, not the ordinary
comptime host adapter. The selected `build: fn` must explicitly declare both
`Net` and `Time.Wait`. Jet checks every DNS answer against the existing public
destination rule (including IPv4-mapped IPv6) and passes only that checked answer
set to the connection leaf. The leaf performs no second DNS lookup and retains
the original host for HTTP Host and TLS SNI/certificate verification.

Fetch policy remains source-owned: a 30-second transport deadline, a 64-MiB
body bound, SHA-256 verification before UTF-8 decoding, and the existing E3413/
E3414 diagnostics. Status codes do not change which bytes are pinned: redirects
are not followed, and the original response body is verified for any status.
Missing build transport or execution/session providers remains an explicit
refusal rather than a successful empty response.
